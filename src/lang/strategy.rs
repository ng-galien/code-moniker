use tree_sitter::Node;

use crate::core::code_graph::Position;

pub enum NodeShape<'src> {
	Symbol(Symbol<'src>),
	Annotation { kind: &'static [u8] },
	Skip,
	Recurse,
}

pub struct Symbol<'src> {
	pub kind: &'static [u8],
	pub name: &'src [u8],
	pub visibility: &'static [u8],
	pub signature: Option<Vec<u8>>,
	pub body: Option<Node<'src>>,
	pub position: Position,
}

pub trait LangStrategy {
	fn classify<'src>(&self, node: Node<'src>, source: &'src [u8]) -> NodeShape<'src>;
}

#[cfg(test)]
mod tests {
	use super::*;

	struct FakeStrategy;

	impl LangStrategy for FakeStrategy {
		fn classify<'src>(&self, node: Node<'src>, _source: &'src [u8]) -> NodeShape<'src> {
			match node.kind() {
				"line_comment" => NodeShape::Annotation { kind: b"comment" },
				_ => NodeShape::Recurse,
			}
		}
	}

	#[test]
	fn fake_strategy_classifies_line_comment_as_annotation() {
		let s = FakeStrategy;
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"// hi\nfn main() {}";
		let tree = p.parse(src, None).unwrap();
		let mut cursor = tree.root_node().walk();
		let mut saw_comment = false;
		for child in tree.root_node().children(&mut cursor) {
			if let NodeShape::Annotation { kind } = s.classify(child, src) {
				assert_eq!(kind, b"comment");
				saw_comment = true;
			}
		}
		assert!(saw_comment);
	}

	#[test]
	fn fake_strategy_recurses_on_unknown_kinds() {
		let s = FakeStrategy;
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"fn main() {}";
		let tree = p.parse(src, None).unwrap();
		let mut cursor = tree.root_node().walk();
		for child in tree.root_node().children(&mut cursor) {
			assert!(matches!(s.classify(child, src), NodeShape::Recurse));
		}
	}
}
