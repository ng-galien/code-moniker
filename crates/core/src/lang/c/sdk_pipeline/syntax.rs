use tree_sitter::Node;

use crate::lang::callable::{CallableSlot, normalize_type_text};
use crate::lang::tree_util::node_slice;

pub(super) fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.collect::<Vec<_>>()
		.into_iter()
}

// What a C declarator chain names once pointers, arrays and parens unwrap.
pub(super) struct DeclaratorInfo<'tree> {
	pub name_node: Node<'tree>,
	pub name: &'tree [u8],
	pub pointer_depth: usize,
	pub is_function: bool,
	pub parameters: Option<Node<'tree>>,
}

pub(super) fn declarator_info<'tree>(
	declarator: Node<'tree>,
	source: &'tree [u8],
) -> Option<DeclaratorInfo<'tree>> {
	let mut node = declarator;
	let mut pointer_depth = 0usize;
	let mut is_function = false;
	let mut parameters: Option<Node<'tree>> = None;
	let mut through_parens = false;
	loop {
		match node.kind() {
			"pointer_declarator" => {
				pointer_depth += 1;
				if let Some(recovered) = recovered_identifier(node) {
					node = recovered;
					continue;
				}
				node = node.child_by_field_name("declarator")?;
			}
			"array_declarator" => {
				node = node.child_by_field_name("declarator")?;
			}
			"function_declarator" => {
				if let Some(recovered) = recovered_identifier(node) {
					node = recovered;
					continue;
				}
				if parameters.is_none() {
					is_function = !through_parens;
					parameters = node.child_by_field_name("parameters");
				}
				node = node.child_by_field_name("declarator")?;
			}
			"parenthesized_declarator" => {
				through_parens = true;
				is_function = false;
				node = named_children(node).next()?;
			}
			"attributed_declarator" => {
				node = named_children(node).next()?;
			}
			"identifier" | "field_identifier" | "type_identifier" => {
				let name = node_slice(node, source);
				if name.is_empty() {
					return None;
				}
				return Some(DeclaratorInfo {
					name_node: node,
					name,
					pointer_depth,
					is_function,
					parameters,
				});
			}
			_ => return None,
		}
	}
}

pub(super) fn recovered_identifier(node: Node<'_>) -> Option<Node<'_>> {
	let error = named_children(node).find(|child| child.kind() == "ERROR")?;
	first_identifier(error)
}

pub(super) fn first_identifier(node: Node<'_>) -> Option<Node<'_>> {
	if matches!(
		node.kind(),
		"identifier" | "field_identifier" | "type_identifier"
	) {
		return Some(node);
	}
	named_children(node).find_map(first_identifier)
}

// Parameter slots render as name:type with pointer stars appended to the base
// type text; a single bare `void` parameter collapses to an empty slot list.
pub(super) fn parameter_slots(params: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let mut out = Vec::new();
	for param in named_children(params) {
		match param.kind() {
			"parameter_declaration" => out.push(parameter_slot(param, source)),
			"variadic_parameter" => out.push(CallableSlot {
				name: Vec::new(),
				r#type: b"...".to_vec(),
			}),
			_ => {}
		}
	}
	if out.len() == 1 && out[0].name.is_empty() && out[0].r#type == b"void" {
		return Vec::new();
	}
	out
}

fn parameter_slot(param: Node<'_>, source: &[u8]) -> CallableSlot {
	let base = param
		.child_by_field_name("type")
		.map(|ty| node_slice(ty, source))
		.unwrap_or_default();
	let mut ty = normalize_type_text(std::str::from_utf8(base).unwrap_or_default());
	let declarator = param.child_by_field_name("declarator");
	if let Some(info) = declarator.and_then(|decl| declarator_info(decl, source)) {
		ty.extend(std::iter::repeat_n(b'*', info.pointer_depth));
		return CallableSlot {
			name: info.name.to_vec(),
			r#type: ty,
		};
	}
	if declarator.is_some_and(|decl| decl.kind().starts_with("abstract_pointer")) {
		ty.push(b'*');
	}
	CallableSlot {
		name: Vec::new(),
		r#type: ty,
	}
}

pub(super) fn is_static(node: Node<'_>, source: &[u8]) -> bool {
	let mut cursor = node.walk();
	node.children(&mut cursor).any(|child| {
		child.kind() == "storage_class_specifier" && node_slice(child, source) == b"static"
	})
}

pub(super) fn visibility_for(node: Node<'_>, source: &[u8]) -> &'static [u8] {
	if is_static(node, source) {
		super::kinds::VIS_MODULE
	} else {
		super::kinds::VIS_PUBLIC
	}
}

pub(super) fn argument_count(args: Node<'_>) -> usize {
	named_children(args).count()
}

pub(super) fn strip_header_suffix(name: &str) -> &str {
	name.strip_suffix(".h").unwrap_or(name)
}

pub(super) fn receiver_hint_bytes<'src>(operand: Node<'src>, source: &'src [u8]) -> &'src [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SUBSCRIPT};
	match operand.kind() {
		"identifier" => node_slice(operand, source),
		"field_expression" | "field_identifier" => HINT_MEMBER,
		"call_expression" => HINT_CALL,
		"subscript_expression" => HINT_SUBSCRIPT,
		_ => b"",
	}
}
