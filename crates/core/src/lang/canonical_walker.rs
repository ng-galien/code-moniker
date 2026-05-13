use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs, RefAttrs};
use crate::core::moniker::Moniker;

use crate::lang::callable::extend_segment_u32;
use crate::lang::strategy::{LangStrategy, NodeShape};

pub struct CanonicalWalker<'a, S: LangStrategy> {
	pub strategy: &'a S,
	pub source: &'a [u8],
}

struct PendingAnnotation {
	kind: &'static [u8],
	start_byte: u32,
	end_byte: u32,
	end_row: usize,
}

impl<'a, S: LangStrategy> CanonicalWalker<'a, S> {
	pub fn new(strategy: &'a S, source: &'a [u8]) -> Self {
		Self { strategy, source }
	}

	pub fn walk(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		let mut pending: Option<PendingAnnotation> = None;
		for child in node.children(&mut cursor) {
			match self.strategy.classify(child, scope, self.source, graph) {
				NodeShape::Annotation { kind } => {
					self.extend_or_flush(&mut pending, kind, child, scope, graph);
				}
				NodeShape::Symbol(sym) => {
					self.flush_pending(&mut pending, scope, graph);
					self.emit_symbol(child, scope, sym, graph);
				}
				NodeShape::Skip => self.flush_pending(&mut pending, scope, graph),
				NodeShape::Recurse => {
					self.flush_pending(&mut pending, scope, graph);
					self.walk(child, scope, graph);
				}
			}
		}
		self.flush_pending(&mut pending, scope, graph);
	}

	fn extend_or_flush(
		&self,
		pending: &mut Option<PendingAnnotation>,
		kind: &'static [u8],
		child: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let start_row = child.start_position().row;
		let end_row = child.end_position().row;
		let start_byte = child.start_byte() as u32;
		let end_byte = child.end_byte() as u32;
		if let Some(p) = pending.as_mut() {
			if p.kind == kind && start_row <= p.end_row + 1 {
				p.end_byte = end_byte;
				p.end_row = end_row;
				return;
			}
			self.emit_annotation_range(p.kind, p.start_byte, p.end_byte, scope, graph);
		}
		*pending = Some(PendingAnnotation {
			kind,
			start_byte,
			end_byte,
			end_row,
		});
	}

	fn flush_pending(
		&self,
		pending: &mut Option<PendingAnnotation>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if let Some(p) = pending.take() {
			self.emit_annotation_range(p.kind, p.start_byte, p.end_byte, scope, graph);
		}
	}

	pub fn dispatch(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		match self.strategy.classify(node, scope, self.source, graph) {
			NodeShape::Annotation { kind } => {
				self.emit_annotation_range(
					kind,
					node.start_byte() as u32,
					node.end_byte() as u32,
					scope,
					graph,
				);
			}
			NodeShape::Symbol(sym) => {
				self.emit_symbol(node, scope, sym, graph);
			}
			NodeShape::Skip => {}
			NodeShape::Recurse => self.walk(node, scope, graph),
		}
	}

	fn emit_annotation_range(
		&self,
		kind: &'static [u8],
		start_byte: u32,
		end_byte: u32,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let m = extend_segment_u32(scope, kind, start_byte);
		let _ = graph.add_def(m, kind, scope, Some((start_byte, end_byte)));
	}

	fn emit_symbol(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		sym: crate::lang::strategy::Symbol<'_>,
		graph: &mut CodeGraph,
	) {
		let crate::lang::strategy::Symbol {
			moniker: m,
			kind,
			visibility,
			signature,
			body,
			position,
			annotated_by,
		} = sym;

		let attrs = DefAttrs {
			visibility,
			signature: signature.as_deref().unwrap_or_default(),
			..DefAttrs::default()
		};
		let added = graph
			.add_def_attrs(m.clone(), kind, scope, Some(position), &attrs)
			.is_ok();
		if !added {
			return;
		}

		for r in annotated_by {
			let attrs = RefAttrs {
				confidence: r.confidence,
				receiver_hint: r.receiver_hint,
				alias: r.alias,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(&m, r.target, r.kind, Some(r.position), &attrs);
		}

		if let Some(body_node) = body {
			self.strategy
				.before_body(node, kind, &m, self.source, graph);
			self.walk(body_node, &m, graph);
			self.strategy.after_body(kind, &m);
		}

		self.strategy
			.on_symbol_emitted(node, kind, &m, self.source, graph);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::moniker::MonikerBuilder;
	use crate::lang::strategy::{NodeShape, Symbol};

	struct RustToyStrategy;

	impl LangStrategy for RustToyStrategy {
		fn classify<'src>(
			&self,
			node: Node<'src>,
			scope: &Moniker,
			source: &'src [u8],
			_graph: &mut CodeGraph,
		) -> NodeShape<'src> {
			match node.kind() {
				"line_comment" | "block_comment" => NodeShape::Annotation { kind: b"comment" },
				"struct_item" => {
					let Some(name) = node.child_by_field_name("name") else {
						return NodeShape::Recurse;
					};
					let bytes = &source[name.start_byte()..name.end_byte()];
					let moniker = MonikerBuilder::from_view(scope.as_view())
						.segment(b"struct", bytes)
						.build();
					NodeShape::Symbol(Symbol {
						moniker,
						kind: b"struct",
						visibility: b"public",
						signature: None,
						body: node.child_by_field_name("body"),
						position: (node.start_byte() as u32, node.end_byte() as u32),
						annotated_by: Vec::new(),
					})
				}
				"function_item" => {
					let Some(name) = node.child_by_field_name("name") else {
						return NodeShape::Recurse;
					};
					let bytes = &source[name.start_byte()..name.end_byte()];
					let moniker = MonikerBuilder::from_view(scope.as_view())
						.segment(b"fn", bytes)
						.build();
					NodeShape::Symbol(Symbol {
						moniker,
						kind: b"fn",
						visibility: b"public",
						signature: None,
						body: node.child_by_field_name("body"),
						position: (node.start_byte() as u32, node.end_byte() as u32),
						annotated_by: Vec::new(),
					})
				}
				_ => NodeShape::Recurse,
			}
		}
	}

	fn anchor() -> Moniker {
		MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"rs")
			.segment(b"module", b"toy")
			.build()
	}

	#[test]
	fn canonical_walker_emits_struct_and_fn_via_strategy() {
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"pub struct Foo;\npub fn bar() {}";
		let tree = p.parse(src, None).unwrap();

		let root = anchor();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let w = CanonicalWalker::new(&RustToyStrategy, src);
		w.walk(tree.root_node(), &root, &mut g);

		let kinds: Vec<&[u8]> = g.defs().map(|d| d.kind.as_slice()).collect();
		assert!(kinds.contains(&b"struct".as_slice()));
		assert!(kinds.contains(&b"fn".as_slice()));
	}

	#[test]
	fn canonical_walker_emits_comments_at_top_level() {
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"// hi\npub struct Foo;";
		let tree = p.parse(src, None).unwrap();

		let root = anchor();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let w = CanonicalWalker::new(&RustToyStrategy, src);
		w.walk(tree.root_node(), &root, &mut g);

		assert_eq!(g.defs().filter(|d| d.kind == b"comment").count(), 1);
	}

	#[test]
	fn canonical_walker_recurses_into_struct_body_and_finds_inner_comments() {
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"pub struct Foo {\n    // hi\n    x: i32,\n}";
		let tree = p.parse(src, None).unwrap();

		let root = anchor();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let w = CanonicalWalker::new(&RustToyStrategy, src);
		w.walk(tree.root_node(), &root, &mut g);

		let comment_under_struct = g.defs().filter(|d| d.kind == b"comment").any(|d| {
			d.moniker
				.as_view()
				.segments()
				.any(|s| s.kind == b"struct" && s.name == b"Foo")
		});
		assert!(
			comment_under_struct,
			"comment inside struct body should be re-parented onto the struct"
		);
	}

	#[test]
	fn canonical_walker_collapses_consecutive_line_comments_into_one_def() {
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"// a\n// b\n// c\npub struct Foo;";
		let tree = p.parse(src, None).unwrap();

		let root = anchor();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let w = CanonicalWalker::new(&RustToyStrategy, src);
		w.walk(tree.root_node(), &root, &mut g);

		let comments: Vec<_> = g.defs().filter(|d| d.kind == b"comment").collect();
		assert_eq!(
			comments.len(),
			1,
			"three adjacent line comments collapse to one def"
		);
		let pos = comments[0].position.expect("comment has a position");
		assert_eq!(
			&src[pos.0 as usize..pos.1 as usize],
			b"// a\n// b\n// c".as_slice(),
			"collapsed span covers the whole run"
		);
	}

	#[test]
	fn canonical_walker_splits_comments_separated_by_blank_line() {
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"// a\n// b\n\n// c\npub struct Foo;";
		let tree = p.parse(src, None).unwrap();

		let root = anchor();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let w = CanonicalWalker::new(&RustToyStrategy, src);
		w.walk(tree.root_node(), &root, &mut g);

		assert_eq!(
			g.defs().filter(|d| d.kind == b"comment").count(),
			2,
			"a blank line breaks the run"
		);
	}

	#[test]
	fn canonical_walker_splits_comments_separated_by_code() {
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"// a\npub struct Foo;\n// b\npub struct Bar;";
		let tree = p.parse(src, None).unwrap();

		let root = anchor();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let w = CanonicalWalker::new(&RustToyStrategy, src);
		w.walk(tree.root_node(), &root, &mut g);

		assert_eq!(
			g.defs().filter(|d| d.kind == b"comment").count(),
			2,
			"code between two comments forces two separate defs"
		);
	}

	#[test]
	fn canonical_walker_does_not_drop_comments_in_mod_inline_position() {
		let mut p = tree_sitter::Parser::new();
		p.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
		let src = b"pub mod inner {\n    // inside\n    pub struct InnerStruct;\n}";
		let tree = p.parse(src, None).unwrap();

		let root = anchor();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let w = CanonicalWalker::new(&RustToyStrategy, src);
		w.walk(tree.root_node(), &root, &mut g);

		assert_eq!(
			g.defs().filter(|d| d.kind == b"comment").count(),
			1,
			"default-recurse must reach into mod_item; comment inside must be emitted"
		);
		assert!(
			g.defs().any(|d| d.kind == b"struct"),
			"the inner struct must also be emitted because the walker recursed"
		);
	}
}
