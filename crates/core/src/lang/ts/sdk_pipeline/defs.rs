use std::collections::HashMap;

use tree_sitter::Node;

use crate::core::code_graph::DefAttrs;
use crate::core::moniker::Moniker;
use crate::lang::callable::{callable_segment_slots, extend_segment};
use crate::lang::sdk::Namespace;
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::canonicalize::callable_param_slots;
use super::syntax::is_callable_kind;

#[derive(Clone)]
pub(super) struct CallableEntry {
	pub(super) kind: &'static [u8],
	pub(super) segment: Vec<u8>,
}

pub(super) fn namespace_for_kind(_kind: &'static [u8]) -> Namespace {
	Namespace::Unified
}

pub(super) fn visibility_attr(value: &[u8]) -> &'static [u8] {
	match value {
		b"public" => kinds::VIS_PUBLIC,
		b"protected" => kinds::VIS_PROTECTED,
		b"private" => kinds::VIS_PRIVATE,
		b"module" => kinds::VIS_MODULE,
		b"none" | b"" => kinds::VIS_NONE,
		_ => kinds::VIS_NONE,
	}
}

pub(super) fn callable_metadata(
	kind: &'static [u8],
	name: &[u8],
	attrs: &DefAttrs<'_>,
) -> (Vec<u8>, Option<usize>) {
	if !attrs.call_name.is_empty() || attrs.call_arity.is_some() {
		return (attrs.call_name.to_vec(), attrs.call_arity);
	}
	if !is_callable_kind(kind) {
		return (Vec::new(), None);
	}
	let bare = crate::core::moniker::query::bare_callable_name(name).to_vec();
	(bare, callable_arity(name))
}

pub(super) fn callable_arity(name: &[u8]) -> Option<usize> {
	let open = name.iter().position(|b| *b == b'(')?;
	let close = name.iter().rposition(|b| *b == b')')?;
	if close <= open + 1 {
		return Some(0);
	}
	Some(name[open + 1..close].split(|b| *b == b',').count())
}

pub(super) fn function_decl_info<'src>(
	node: Node<'src>,
	source: &'src [u8],
) -> Option<(&'src [u8], Vec<crate::lang::callable::CallableSlot>)> {
	if !matches!(
		node.kind(),
		"function_declaration" | "generator_function_declaration"
	) {
		return None;
	}
	let name_node = node.child_by_field_name("name")?;
	let name = node_slice(name_node, source);
	Some((name, callable_param_slots(node, source)))
}

pub(super) fn collect_callable_table<'src>(
	root: Node<'src>,
	source: &'src [u8],
	module: &Moniker,
	out: &mut HashMap<(Moniker, Vec<u8>), CallableEntry>,
) {
	visit_top_level(root, |child| match child.kind() {
		"function_declaration" | "generator_function_declaration" => {
			if let Some((name, slots)) = function_decl_info(child, source) {
				out.insert(
					(module.clone(), name.to_vec()),
					CallableEntry {
						kind: kinds::FUNCTION,
						segment: callable_segment_slots(name, &slots),
					},
				);
			}
		}
		"lexical_declaration" | "variable_declaration" => {
			let mut nc = child.walk();
			for decl in child.named_children(&mut nc) {
				if decl.kind() != "variable_declarator" {
					continue;
				}
				let Some(name_node) = decl.child_by_field_name("name") else {
					continue;
				};
				if name_node.kind() != "identifier" {
					continue;
				}
				let name = node_slice(name_node, source);
				let (kind, seg) = match decl.child_by_field_name("value") {
					Some(v) if matches!(v.kind(), "arrow_function" | "function_expression") => {
						let slots = callable_param_slots(v, source);
						(kinds::FUNCTION, callable_segment_slots(name, &slots))
					}
					_ => (kinds::CONST, name.to_vec()),
				};
				out.insert(
					(module.clone(), name.to_vec()),
					CallableEntry { kind, segment: seg },
				);
			}
		}
		_ => {}
	});
}

pub(super) fn collect_type_table<'src>(
	root: Node<'src>,
	source: &'src [u8],
	module: &Moniker,
	out: &mut HashMap<Vec<u8>, Moniker>,
) {
	visit_top_level(root, |child| match child.kind() {
		"class_declaration" | "abstract_class_declaration" => {
			if let Some(name_node) = child.child_by_field_name("name") {
				let name = node_slice(name_node, source);
				out.insert(name.to_vec(), extend_segment(module, kinds::CLASS, name));
			}
		}
		"interface_declaration" => {
			if let Some(name_node) = child.child_by_field_name("name") {
				let name = node_slice(name_node, source);
				out.insert(
					name.to_vec(),
					extend_segment(module, kinds::INTERFACE, name),
				);
			}
		}
		"enum_declaration" => {
			if let Some(name_node) = child.child_by_field_name("name") {
				let name = node_slice(name_node, source);
				out.insert(name.to_vec(), extend_segment(module, kinds::ENUM, name));
			}
		}
		"type_alias_declaration" => {
			if let Some(name_node) = child.child_by_field_name("name") {
				let name = node_slice(name_node, source);
				out.insert(name.to_vec(), extend_segment(module, kinds::TYPE, name));
			}
		}
		_ => {}
	});
}

fn visit_top_level<'src, F: FnMut(Node<'src>)>(root: Node<'src>, mut f: F) {
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		match child.kind() {
			"export_statement" => {
				let mut ec = child.walk();
				for inner in child.named_children(&mut ec) {
					f(inner);
				}
			}
			_ => f(child),
		}
	}
}

pub(super) fn collect_export_ranges(root: Node<'_>) -> Vec<(u32, u32)> {
	let mut out = Vec::new();
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() == "export_statement" {
			out.push(node_position(child));
		}
	}
	out
}
