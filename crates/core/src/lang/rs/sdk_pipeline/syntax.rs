use tree_sitter::Node;

use crate::lang::tree_util::node_slice;

pub(super) fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.collect::<Vec<_>>()
		.into_iter()
}

pub(super) fn is_test_function(node: Node<'_>, source: &[u8]) -> bool {
	if node
		.utf8_text(source)
		.is_ok_and(|text| text.contains("#[test]"))
	{
		return true;
	}
	let mut sibling = node.prev_named_sibling();
	while let Some(previous) = sibling {
		if previous.kind() != "attribute_item" {
			break;
		}
		if previous
			.utf8_text(source)
			.is_ok_and(|text| text.contains("test"))
		{
			return true;
		}
		sibling = previous.prev_named_sibling();
	}
	let mut cursor = node.walk();
	node.children(&mut cursor)
		.take_while(|child| child.kind() != "fn")
		.filter(|child| child.kind() == "attribute_item")
		.any(|attribute| {
			attribute
				.utf8_text(source)
				.map(|text| text.contains("test"))
				.unwrap_or(false)
		})
}

pub(super) fn language_macro_variants(text: &str) -> Vec<String> {
	text.lines()
		.filter_map(|line| line.split_once("=>").map(|(left, _)| left.trim()))
		.filter(|candidate| {
			!candidate.is_empty()
				&& candidate
					.chars()
					.next()
					.is_some_and(|first| first.is_ascii_uppercase())
		})
		.map(str::to_string)
		.collect()
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
