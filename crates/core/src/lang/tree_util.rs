use tree_sitter::Node;

pub(crate) fn node_position(node: Node<'_>) -> (u32, u32) {
	(node.start_byte() as u32, node.end_byte() as u32)
}

pub(crate) fn node_slice<'src>(node: Node<'src>, source: &'src [u8]) -> &'src [u8] {
	&source[node.start_byte()..node.end_byte().min(source.len())]
}

pub(crate) fn find_named_child<'tree>(parent: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
	let mut cursor = parent.walk();
	parent
		.named_children(&mut cursor)
		.find(|c| c.kind() == kind)
}

pub(crate) fn find_descendant<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
	if node.kind() == kind {
		return Some(node);
	}
	let mut cursor = node.walk();
	for c in node.named_children(&mut cursor) {
		if let Some(d) = find_descendant(c, kind) {
			return Some(d);
		}
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parse_rust(src: &str) -> tree_sitter::Tree {
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		p.parse(src, None).unwrap()
	}

	#[test]
	fn node_position_returns_byte_range() {
		let tree = parse_rust("pub fn foo() {}");
		let root = tree.root_node();
		assert_eq!(node_position(root), (0, 15));
	}

	#[test]
	fn node_slice_borrows_from_source() {
		let src = b"pub fn foo() {}";
		let tree = parse_rust(std::str::from_utf8(src).unwrap());
		let root = tree.root_node();
		let slice = node_slice(root, src);
		assert_eq!(slice, b"pub fn foo() {}");
		assert_eq!(slice.as_ptr(), src.as_ptr());
	}

	#[test]
	fn find_named_child_returns_first_match() {
		let tree = parse_rust("pub fn a() {} pub fn b() {}");
		let root = tree.root_node();
		let first = find_named_child(root, "function_item").unwrap();
		assert_eq!(first.start_byte(), 0);
	}

	#[test]
	fn find_named_child_returns_none_when_absent() {
		let tree = parse_rust("pub fn a() {}");
		let root = tree.root_node();
		assert!(find_named_child(root, "struct_item").is_none());
	}

	#[test]
	fn find_descendant_is_recursive() {
		let tree = parse_rust("pub fn a() { let x = 1; }");
		let root = tree.root_node();
		assert!(find_descendant(root, "let_declaration").is_some());
	}

	#[test]
	fn find_descendant_matches_self() {
		let tree = parse_rust("pub fn a() {}");
		let root = tree.root_node();
		assert_eq!(
			find_descendant(root, "source_file").map(|n| n.start_byte()),
			Some(0)
		);
	}
}
