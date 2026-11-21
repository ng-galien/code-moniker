use tree_sitter::Node;

use crate::core::code_graph::{Bytes, FunctionLinkage};
use crate::core::moniker::Moniker;
use crate::lang::tree_util::node_slice;

use super::discover::CDiscover;

pub(super) fn record_function_linkage(
	state: &mut CDiscover<'_>,
	moniker: &Moniker,
	node: Node<'_>,
	declarator: Node<'_>,
	name: &[u8],
	internal: bool,
) {
	if internal {
		state.internal_functions.insert(name.to_vec());
	}
	let declaration_only = node.kind() == "declaration";
	let linkage = FunctionLinkage {
		declaration_only,
		signature: if internal {
			None
		} else {
			external_signature(node, declarator, state)
		},
	};
	if declaration_only {
		state
			.function_linkage
			.entry(moniker.clone())
			.or_insert(linkage);
	} else {
		state.function_linkage.insert(moniker.clone(), linkage);
	}
}

pub(super) fn header_guard(root: Node<'_>, source: &[u8]) -> Option<usize> {
	let mut cursor = root.walk();
	let mut top = root
		.named_children(&mut cursor)
		.filter(|child| child.kind() != "comment");
	let guard = top.next()?;
	if guard.kind() != "preproc_ifdef"
		|| guard.child(0)?.kind() != "#ifndef"
		|| top.next().is_some()
		|| guard.child_by_field_name("alternative").is_some()
	{
		return None;
	}
	let mut cursor = guard.walk();
	let mut children = guard
		.named_children(&mut cursor)
		.filter(|child| child.kind() != "comment");
	let name = children.next()?;
	let define = children.next()?;
	let defined = define.child_by_field_name("name")?;
	(name.kind() == "identifier"
		&& define.kind() == "preproc_def"
		&& node_slice(name, source) == node_slice(defined, source))
	.then_some(guard.id())
}

// Only structurally simple types are certified. Tagged pointers are marked
// by the space after "struct", so they require a common header at linkage.
fn external_signature(
	node: Node<'_>,
	declarator: Node<'_>,
	state: &CDiscover<'_>,
) -> Option<Bytes> {
	let source = state.source;
	let parent = node.parent()?;
	let recovered = node.kind() == "function_definition"
		&& parent.kind() == "ERROR"
		&& parent
			.parent()
			.is_some_and(|root| root.kind() == "translation_unit")
		&& node.child_by_field_name("body").is_some();
	if !(parent.kind() == "translation_unit"
		|| recovered
		|| (node.kind() == "declaration" && state.header_guard == Some(parent.id())))
		|| declarator.has_error()
		|| !children_match(node, |child| match child.kind() {
			"storage_class_specifier" => node_slice(child, source) == b"extern",
			"primitive_type"
			| "sized_type_specifier"
			| "struct_specifier"
			| "function_declarator"
			| "pointer_declarator"
			| "compound_statement"
			| "comment" => true,
			_ => false,
		}) {
		return None;
	}
	let (function, return_pointers) = plain_pointer_chain(declarator)?;
	let function = function.filter(|node| node.kind() == "function_declarator")?;
	let name = function.child_by_field_name("declarator")?;
	if name.kind() != "identifier" {
		return None;
	}
	let params = function.child_by_field_name("parameters")?;
	if !children_match(function, |child| {
		child == name || child == params || child.kind() == "comment"
	}) {
		return None;
	}
	let mut signature = Vec::with_capacity(24);
	if recovered {
		signature.push(super::super::RECOVERED_BODY_SIGNATURE);
	}
	append_linkage_type(&mut signature, node, source, return_pointers)?;
	signature.push(b'(');
	let mut cursor = params.walk();
	let mut parameters = params
		.named_children(&mut cursor)
		.filter(|child| child.kind() != "comment")
		.peekable();
	let first = parameters.next()?;
	let parameters_start = signature.len();
	append_parameter_type(&mut signature, first, source)?;
	if &signature[parameters_start..] == b"void"
		&& first.child_by_field_name("declarator").is_none()
		&& parameters.peek().is_none()
	{
		signature.truncate(parameters_start);
	} else {
		for param in parameters {
			signature.push(b',');
			append_parameter_type(&mut signature, param, source)?;
		}
	}
	signature.push(b')');
	Some(signature.into())
}

fn append_parameter_type(signature: &mut Vec<u8>, node: Node<'_>, source: &[u8]) -> Option<()> {
	if node.kind() != "parameter_declaration"
		|| !children_match(node, |child| {
			matches!(
				child.kind(),
				"primitive_type"
					| "sized_type_specifier"
					| "struct_specifier"
					| "identifier" | "pointer_declarator"
					| "abstract_pointer_declarator"
					| "comment"
			)
		}) {
		return None;
	}
	let pointers = match node.child_by_field_name("declarator") {
		Some(declarator) => {
			let (base, pointers) = plain_pointer_chain(declarator)?;
			if base.is_some_and(|name| name.kind() != "identifier") {
				return None;
			}
			pointers
		}
		None => 0,
	};
	append_linkage_type(signature, node, source, pointers)
}

fn plain_pointer_chain(mut node: Node<'_>) -> Option<(Option<Node<'_>>, usize)> {
	let mut pointers = 0;
	while matches!(
		node.kind(),
		"pointer_declarator" | "abstract_pointer_declarator"
	) {
		let inner = node.child_by_field_name("declarator");
		if !children_match(node, |child| {
			Some(child) == inner || child.kind() == "comment"
		}) {
			return None;
		}
		pointers += 1;
		let Some(inner) = inner else {
			return (node.kind() == "abstract_pointer_declarator").then_some((None, pointers));
		};
		node = inner;
	}
	Some((Some(node), pointers))
}

fn append_linkage_type(
	signature: &mut Vec<u8>,
	node: Node<'_>,
	source: &[u8],
	pointers: usize,
) -> Option<()> {
	let ty = node.child_by_field_name("type")?;
	match ty.kind() {
		"primitive_type" | "sized_type_specifier" => signature.extend(
			node_slice(ty, source)
				.iter()
				.copied()
				.filter(|byte| !byte.is_ascii_whitespace()),
		),
		"struct_specifier" if pointers > 0 && ty.child_by_field_name("body").is_none() => {
			let tag = ty.child_by_field_name("name")?;
			if !children_match(ty, |child| child == tag || child.kind() == "comment") {
				return None;
			}
			signature.extend_from_slice(b"struct ");
			signature.extend_from_slice(node_slice(tag, source));
		}
		_ => return None,
	}
	signature.extend(std::iter::repeat_n(b'*', pointers));
	Some(())
}

fn children_match<'tree>(node: Node<'tree>, predicate: impl FnMut(Node<'tree>) -> bool) -> bool {
	let mut cursor = node.walk();
	node.named_children(&mut cursor).all(predicate)
}
