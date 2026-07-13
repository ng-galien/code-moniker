use tree_sitter::Node;

use crate::lang::callable::{CallableSlot, normalize_type_text};
use crate::lang::tree_util::node_slice;

pub(super) fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.collect::<Vec<_>>()
		.into_iter()
}

pub(super) fn function_param_slots(node: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let Some(params) = node.child_by_field_name("parameters") else {
		return Vec::new();
	};
	flatten_param_slots(params, source)
}

pub(super) fn flatten_param_slots(params: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let mut out = Vec::new();
	for child in named_children(params) {
		match child.kind() {
			"parameter_declaration" => {
				let ty = child
					.child_by_field_name("type")
					.and_then(|n| n.utf8_text(source).ok())
					.map(normalize_type_text)
					.unwrap_or_default();
				let names = parameter_name_bytes(child, source);
				if names.is_empty() {
					out.push(CallableSlot {
						name: Vec::new(),
						r#type: ty,
					});
				} else {
					for name in names {
						out.push(CallableSlot {
							name,
							r#type: ty.clone(),
						});
					}
				}
			}
			"variadic_parameter_declaration" => {
				out.push(CallableSlot {
					name: Vec::new(),
					r#type: b"...".to_vec(),
				});
			}
			_ => {}
		}
	}
	out
}

pub(super) fn parameter_name_bytes(decl: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let mut out = Vec::new();
	for child in named_children(decl) {
		if child.kind() == "identifier" {
			let name = node_slice(child, source);
			if !name.is_empty() && name != b"_" {
				out.push(name.to_vec());
			}
		}
	}
	out
}

pub(super) fn receiver_type_name<'a>(receiver: Node<'a>, source: &'a [u8]) -> Option<&'a [u8]> {
	let param = named_children(receiver).next()?;
	if param.kind() != "parameter_declaration" {
		return None;
	}
	let type_node = param.child_by_field_name("type")?;
	extract_type_name(type_node, source)
}

pub(super) fn extract_type_name<'a>(node: Node<'a>, source: &'a [u8]) -> Option<&'a [u8]> {
	match node.kind() {
		"type_identifier" => Some(node_slice(node, source)),
		"pointer_type" => {
			let inner = named_children(node).next()?;
			extract_type_name(inner, source)
		}
		"generic_type" => {
			let inner = node.child_by_field_name("type")?;
			extract_type_name(inner, source)
		}
		_ => None,
	}
}

pub(super) fn struct_field_list(struct_node: Node<'_>) -> Option<Node<'_>> {
	named_children(struct_node).find(|child| child.kind() == "field_declaration_list")
}

pub(super) fn strip_string_quotes(raw: &str) -> &str {
	let trimmed = raw
		.strip_prefix('"')
		.and_then(|s| s.strip_suffix('"'))
		.or_else(|| raw.strip_prefix('`').and_then(|s| s.strip_suffix('`')));
	trimmed.unwrap_or(raw)
}

pub(super) fn spec_children<'tree>(node: Node<'tree>, spec_kind: &str) -> Vec<Node<'tree>> {
	let mut out = Vec::new();
	for child in named_children(node) {
		match child.kind() {
			kind if kind == spec_kind => out.push(child),
			"var_spec_list" | "const_spec_list" => {
				for spec in named_children(child) {
					if spec.kind() == spec_kind {
						out.push(spec);
					}
				}
			}
			_ => {}
		}
	}
	out
}

pub(super) fn argument_count(args: Node<'_>) -> usize {
	named_children(args).count()
}

pub(super) fn visibility_from_name(name: &[u8]) -> &'static [u8] {
	match name.first().copied() {
		Some(b) if b.is_ascii_uppercase() => super::kinds::VIS_PUBLIC,
		_ => super::kinds::VIS_MODULE,
	}
}

pub(super) fn receiver_hint_bytes<'src>(operand: Node<'src>, source: &'src [u8]) -> &'src [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SUBSCRIPT};
	match operand.kind() {
		"identifier" => node_slice(operand, source),
		"selector_expression" | "field_identifier" => HINT_MEMBER,
		"call_expression" => HINT_CALL,
		"index_expression" => HINT_SUBSCRIPT,
		_ => b"",
	}
}
