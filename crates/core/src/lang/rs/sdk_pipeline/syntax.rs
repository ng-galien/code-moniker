use tree_sitter::Node;

use crate::lang::tree_util::node_slice;

pub(super) fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.collect::<Vec<_>>()
		.into_iter()
}

pub(super) fn children<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
	let mut cursor = node.walk();
	node.children(&mut cursor).collect()
}

pub(super) fn is_test_function(node: Node<'_>, source: &[u8]) -> bool {
	if previous_attributes(node)
		.into_iter()
		.any(|attribute| is_bare_test_attribute(attribute, source))
	{
		return true;
	}
	children(node)
		.into_iter()
		.take_while(|child| child.kind() != "fn")
		.any(|child| is_bare_test_attribute(child, source))
}

fn previous_attributes(node: Node<'_>) -> Vec<Node<'_>> {
	let mut out = Vec::new();
	let mut sibling = node.prev_named_sibling();
	while let Some(previous) = sibling {
		if previous.kind() != "attribute_item" {
			break;
		}
		out.push(previous);
		sibling = previous.prev_named_sibling();
	}
	out
}

pub(super) fn is_bare_test_attribute(node: Node<'_>, source: &[u8]) -> bool {
	node.kind() == "attribute_item" && path_pieces(node, source) == vec![b"test".to_vec()]
}

pub(super) fn language_macro_variants(node: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let Some(body) = token_tree_body(node) else {
		return Vec::new();
	};
	children(body)
		.windows(2)
		.filter_map(|pair| {
			let candidate = pair[0];
			(candidate.kind() == "identifier" && pair[1].kind() == "=>")
				.then(|| node_slice(candidate, source).to_vec())
		})
		.filter(|candidate| {
			candidate
				.first()
				.is_some_and(|first| first.is_ascii_uppercase())
		})
		.collect()
}

pub(super) fn token_tree_body(node: Node<'_>) -> Option<Node<'_>> {
	named_children(node).find(|child| child.kind() == "token_tree")
}

pub(super) fn should_skip_binding(name: &[u8]) -> bool {
	name == b"_" || name.is_empty()
}

pub(super) fn path_pieces(node: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let mut out = Vec::new();
	collect_path_pieces(node, source, &mut out);
	out
}

fn collect_path_pieces(node: Node<'_>, source: &[u8], out: &mut Vec<Vec<u8>>) {
	match node.kind() {
		"identifier" | "type_identifier" | "self" | "super" | "crate" => {
			out.push(node_slice(node, source).to_vec());
		}
		_ => {
			for child in named_children(node) {
				collect_path_pieces(child, source, out);
			}
		}
	}
}
