use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, Position};
use crate::core::moniker::Moniker;

pub enum NodeShape<'src> {
	Symbol(Symbol<'src>),
	Annotation { kind: &'static [u8] },
	Skip,
	Recurse,
}

pub struct Symbol<'src> {
	pub moniker: Moniker,
	pub kind: &'static [u8],
	pub visibility: &'static [u8],
	pub signature: Option<Vec<u8>>,
	pub body: Option<Node<'src>>,
	pub position: Position,
	pub annotated_by: Vec<RefSpec>,
}

pub struct RefSpec {
	pub kind: &'static [u8],
	pub target: Moniker,
	pub confidence: &'static [u8],
	pub position: Position,
	pub receiver_hint: &'static [u8],
	pub alias: &'static [u8],
}

pub trait LangStrategy {
	fn classify<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
	) -> NodeShape<'src>;

	#[allow(unused_variables)]
	fn before_body(
		&self,
		node: Node<'_>,
		kind: &[u8],
		moniker: &Moniker,
		source: &[u8],
		graph: &mut CodeGraph,
	) {
	}

	#[allow(unused_variables)]
	fn after_body(&self, kind: &[u8], moniker: &Moniker) {}

	#[allow(unused_variables)]
	fn on_symbol_emitted(
		&self,
		node: Node<'_>,
		sym_kind: &[u8],
		sym_moniker: &Moniker,
		source: &[u8],
		graph: &mut CodeGraph,
	) {
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::moniker::MonikerBuilder;

	struct FakeStrategy;

	impl LangStrategy for FakeStrategy {
		fn classify<'src>(
			&self,
			node: Node<'src>,
			_scope: &Moniker,
			_source: &'src [u8],
			_graph: &mut CodeGraph,
		) -> NodeShape<'src> {
			match node.kind() {
				"line_comment" => NodeShape::Annotation { kind: b"comment" },
				_ => NodeShape::Recurse,
			}
		}
	}

	fn anchor() -> Moniker {
		MonikerBuilder::new().project(b"app").build()
	}

	#[test]
	fn fake_strategy_classifies_line_comment_as_annotation() {
		let s = FakeStrategy;
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"// hi\nfn main() {}";
		let tree = p.parse(src, None).unwrap();
		let mut cursor = tree.root_node().walk();
		let scope = anchor();
		let mut g = CodeGraph::new(scope.clone(), b"module");
		let mut saw_comment = false;
		for child in tree.root_node().children(&mut cursor) {
			if let NodeShape::Annotation { kind } = s.classify(child, &scope, src, &mut g) {
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
		let scope = anchor();
		let mut g = CodeGraph::new(scope.clone(), b"module");
		for child in tree.root_node().children(&mut cursor) {
			assert!(matches!(
				s.classify(child, &scope, src, &mut g),
				NodeShape::Recurse
			));
		}
	}
}
