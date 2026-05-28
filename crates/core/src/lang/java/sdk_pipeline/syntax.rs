use tree_sitter::Node;

use crate::lang::tree_util::node_slice;

pub(super) fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.collect::<Vec<_>>()
		.into_iter()
}

pub(super) fn path_pieces(node: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let mut out = Vec::new();
	collect_path_pieces(node, source, &mut out);
	out
}

fn collect_path_pieces(node: Node<'_>, source: &[u8], out: &mut Vec<Vec<u8>>) {
	match node.kind() {
		"identifier" | "type_identifier" => out.push(node_slice(node, source).to_vec()),
		_ => {
			for child in named_children(node) {
				collect_path_pieces(child, source, out);
			}
		}
	}
}

pub(super) fn last_identifier(node: Node<'_>, source: &[u8]) -> Vec<u8> {
	node.child_by_field_name("name")
		.map(|name| node_slice(name, source).to_vec())
		.unwrap_or_else(|| path_pieces(node, source).pop().unwrap_or_default())
}

pub(super) fn type_name(node: Node<'_>, source: &[u8]) -> Option<Vec<u8>> {
	match node.kind() {
		"type_identifier" => Some(node_slice(node, source).to_vec()),
		"scoped_type_identifier" => Some(last_identifier(node, source)),
		"generic_type" => node
			.child_by_field_name("type")
			.or_else(|| {
				named_children(node).find(|child| {
					matches!(child.kind(), "type_identifier" | "scoped_type_identifier")
				})
			})
			.and_then(|ty| type_name(ty, source)),
		"array_type" => node
			.child_by_field_name("element")
			.and_then(|element| type_name(element, source)),
		_ => None,
	}
}

pub(super) fn type_parameters(node: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let Some(params) = node
		.child_by_field_name("type_parameters")
		.or_else(|| named_children(node).find(|child| child.kind() == "type_parameters"))
	else {
		return Vec::new();
	};
	named_children(params)
		.filter(|child| child.kind() == "type_parameter")
		.filter_map(|child| {
			child
				.child_by_field_name("name")
				.or_else(|| named_children(child).find(|inner| inner.kind() == "type_identifier"))
		})
		.map(|name| node_slice(name, source).to_vec())
		.collect()
}
