use tree_sitter::Node;

use crate::core::moniker::Moniker;
use crate::lang::callable::extend_segment;
use crate::lang::tree_util::node_slice;

use super::super::kinds;
use super::discover::JavaDiscover;
use super::syntax::named_children;

pub(super) fn predeclare_types(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	match type_kind(node.kind()) {
		Some(kind) => predeclare_type(state, node, scope, kind),
		None => {
			for child in named_children(node) {
				predeclare_types(state, child, scope);
			}
		}
	}
}

fn predeclare_type(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	kind: &'static [u8],
) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	let type_scope = extend_segment(scope, kind, name);
	state
		.type_table
		.entry(name.to_vec())
		.or_insert_with(|| type_scope.clone());
	if let Some(body) = node.child_by_field_name("body") {
		for child in named_children(body) {
			predeclare_types(state, child, &type_scope);
		}
	}
}

fn type_kind(kind: &str) -> Option<&'static [u8]> {
	match kind {
		"class_declaration" => Some(kinds::CLASS),
		"interface_declaration" => Some(kinds::INTERFACE),
		"enum_declaration" => Some(kinds::ENUM),
		"record_declaration" => Some(kinds::RECORD),
		"annotation_type_declaration" => Some(kinds::ANNOTATION_TYPE),
		_ => None,
	}
}
