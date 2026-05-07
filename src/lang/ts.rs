//! TypeScript parser and extractor.

use tree_sitter::{Language, Node, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::kind_registry::{KindId, KindRegistry, PunctClass};
use crate::core::moniker::{Moniker, MonikerBuilder};

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
	parser
		.set_language(&language)
		.expect("failed to load tree-sitter TypeScript grammar");
	parser
		.parse(source, None)
		.expect("tree-sitter parse returned None on a non-cancelled call")
}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	registry: &mut KindRegistry,
) -> CodeGraph {
	let kinds = TsKinds::new(registry);
	let module = compute_module_moniker(anchor, uri, kinds.path);
	let mut graph = CodeGraph::new(module.clone(), kinds.path);
	let tree = parse(source);
	let walker = Walker {
		source_bytes: source.as_bytes(),
		kinds,
		module: module.clone(),
	};
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

#[derive(Copy, Clone)]
struct TsKinds {
	// Canonical structural kinds — used in moniker bytes so URI
	// roundtrip and `=` equality stay punct-class-stable.
	path: KindId,
	type_canon: KindId,
	method_canon: KindId,

	// Semantic labels — used as `DefRecord.kind` / `RefRecord.kind`
	// metadata and surfaced as text in the SQL API.
	class_label: KindId,
	function_label: KindId,
	import_label: KindId,
}

impl TsKinds {
	fn new(reg: &mut KindRegistry) -> Self {
		Self {
			path: reg.intern("path", PunctClass::Path).unwrap(),
			type_canon: reg.intern("type", PunctClass::Type).unwrap(),
			method_canon: reg.intern("method", PunctClass::Method).unwrap(),
			class_label: reg.intern("class", PunctClass::Type).unwrap(),
			function_label: reg.intern("function", PunctClass::Method).unwrap(),
			import_label: reg.intern("import", PunctClass::Path).unwrap(),
		}
	}
}

struct Walker<'src> {
	source_bytes: &'src [u8],
	kinds: TsKinds,
	module: Moniker,
}

impl<'src> Walker<'src> {
	fn walk(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"class_declaration" => self.handle_class(child, parent, graph),
				"function_declaration" => self.handle_function(child, parent, graph),
				"import_statement" => self.handle_import(child, parent, graph),
				"export_statement" => self.walk(child, parent, graph),
				_ => {}
			}
		}
	}

	fn handle_class(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let class_moniker = extend_segment(parent, self.kinds.type_canon, name.as_bytes());
		let _ = graph.add_def(
			class_moniker.clone(),
			self.kinds.class_label,
			parent,
			Some(node_position(node)),
		);
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_class_body(body, &class_moniker, graph);
		}
	}

	fn walk_class_body(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() == "method_definition" {
				self.handle_method(child, parent, graph);
			}
		}
	}

	fn handle_method(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_method(parent, self.kinds.method_canon, name.as_bytes(), 0);
		let _ = graph.add_def(m, self.kinds.method_canon, parent, Some(node_position(node)));
	}

	fn handle_function(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_method(parent, self.kinds.method_canon, name.as_bytes(), 0);
		let _ = graph.add_def(m, self.kinds.function_label, parent, Some(node_position(node)));
	}

	fn handle_import(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(src_node) = node.child_by_field_name("source") else { return };
		let raw = src_node.utf8_text(self.source_bytes).unwrap_or("");
		let path = raw.trim_matches(|c| c == '"' || c == '\'');
		let target = self.build_import_target(path);
		let _ = graph.add_ref(
			parent,
			target,
			self.kinds.import_label,
			Some(node_position(node)),
		);
	}

	fn build_import_target(&self, path: &str) -> Moniker {
		let view = self.module.as_view();
		MonikerBuilder::new()
			.project(view.project())
			.segment(self.kinds.path, path.as_bytes())
			.build()
	}

	fn field_text(&self, node: Node<'_>, field: &str) -> Option<&'src str> {
		node.child_by_field_name(field)?
			.utf8_text(self.source_bytes)
			.ok()
	}
}

fn compute_module_moniker(anchor: &Moniker, uri: &str, path_kind: KindId) -> Moniker {
	let stem = strip_known_extension(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	for seg in stem.split('/').filter(|s| !s.is_empty()) {
		builder.segment(path_kind, seg.as_bytes());
	}
	builder.build()
}

fn strip_known_extension(uri: &str) -> &str {
	const EXTS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];
	EXTS.iter()
		.find_map(|ext| uri.strip_suffix(ext))
		.unwrap_or(uri)
}

fn extend_segment(parent: &Moniker, kind: KindId, bytes: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, bytes);
	b.build()
}

fn extend_method(parent: &Moniker, kind: KindId, bytes: &[u8], arity: u16) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.method(kind, bytes, arity);
	b.build()
}

fn node_position(node: Node<'_>) -> (u32, u32) {
	(node.start_byte() as u32, node.end_byte() as u32)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn make_anchor() -> (KindRegistry, Moniker, KindId) {
		let mut reg = KindRegistry::new();
		let path = reg.intern("path", PunctClass::Path).unwrap();
		let anchor = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.build();
		(reg, anchor, path)
	}

	// --- parser shim --------------------------------------------------

	#[test]
	fn parse_empty_source_returns_program() {
		let tree = parse("");
		assert_eq!(tree.root_node().kind(), "program");
		assert_eq!(tree.root_node().child_count(), 0);
	}

	#[test]
	fn parse_simple_class_has_class_declaration() {
		let tree = parse("class Foo {}");
		assert_eq!(
			tree.root_node().child(0).unwrap().kind(),
			"class_declaration"
		);
	}

	#[test]
	fn parse_invalid_syntax_marks_errors() {
		assert!(parse("class { ").root_node().has_error());
	}

	// --- extract: empty source ----------------------------------------

	#[test]
	fn extract_empty_source_yields_module_only_graph() {
		let (mut reg, anchor, path) = make_anchor();
		let graph = extract("src/lib/util.ts", "", &anchor, &mut reg);
		assert_eq!(graph.def_count(), 1);
		assert_eq!(graph.ref_count(), 0);

		let expected = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.segment(path, b"src")
			.segment(path, b"lib")
			.segment(path, b"util")
			.build();
		assert_eq!(graph.root(), &expected);
	}

	#[test]
	fn extract_strips_each_known_extension() {
		let (mut reg, anchor, _) = make_anchor();
		for uri in ["foo.ts", "foo.tsx", "foo.js", "foo.jsx", "foo.mjs", "foo.cjs"] {
			let g = extract(uri, "", &anchor, &mut reg);
			let last = g.root().as_view().segments().last().unwrap();
			assert_eq!(last.bytes, b"foo", "extension not stripped on {uri}");
		}
	}

	// --- extract: class declaration -----------------------------------

	#[test]
	fn extract_simple_class_emits_class_def() {
		let (mut reg, anchor, path) = make_anchor();
		let graph = extract("util.ts", "class Foo {}", &anchor, &mut reg);
		assert_eq!(graph.def_count(), 2);

		let type_kid = reg.intern("type", PunctClass::Type).unwrap();
		let foo = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.segment(path, b"util")
			.segment(type_kid, b"Foo")
			.build();
		assert!(graph.contains(&foo));
	}

	#[test]
	fn extract_export_class_descends_into_export_statement() {
		let (mut reg, anchor, _) = make_anchor();
		let graph = extract("util.ts", "export class Foo {}", &anchor, &mut reg);
		assert_eq!(graph.def_count(), 2);
	}

	// --- extract: class with method -----------------------------------

	#[test]
	fn extract_class_with_method_emits_method_def() {
		let (mut reg, anchor, path) = make_anchor();
		let graph = extract("util.ts", "class Foo { bar() {} }", &anchor, &mut reg);
		assert_eq!(graph.def_count(), 3);

		let type_kid = reg.intern("type", PunctClass::Type).unwrap();
		let method_kid = reg.intern("method", PunctClass::Method).unwrap();
		let bar = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.segment(path, b"util")
			.segment(type_kid, b"Foo")
			.method(method_kid, b"bar", 0)
			.build();
		assert!(graph.contains(&bar));
	}

	// --- extract: top-level function ----------------------------------

	#[test]
	fn extract_function_declaration_emits_def() {
		let (mut reg, anchor, path) = make_anchor();
		let graph = extract("util.ts", "function foo() {}", &anchor, &mut reg);
		assert_eq!(graph.def_count(), 2);

		let method_kid = reg.intern("method", PunctClass::Method).unwrap();
		let foo = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.segment(path, b"util")
			.method(method_kid, b"foo", 0)
			.build();
		assert!(graph.contains(&foo));
	}

	// --- extract: import statement -> ref -----------------------------

	#[test]
	fn extract_import_emits_ref_from_module() {
		let (mut reg, anchor, _) = make_anchor();
		let graph = extract(
			"util.ts",
			"import { Bar } from './bar';",
			&anchor,
			&mut reg,
		);
		assert_eq!(graph.ref_count(), 1);

		let r = graph.refs().next().unwrap();
		let import_kid = reg.intern("import", PunctClass::Path).unwrap();
		let path_kid = reg.intern("path", PunctClass::Path).unwrap();
		assert_eq!(r.kind, import_kid, "ref carries the semantic 'import' kind");
		assert_eq!(r.source, 0, "ref attached to the module root");

		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path_kid, b"./bar")
			.build();
		assert_eq!(r.target, target, "import target uses canonical path kind");
	}

	#[test]
	fn extract_import_position_covers_statement() {
		let (mut reg, anchor, _) = make_anchor();
		let source = "import { Bar } from './bar';";
		let graph = extract("util.ts", source, &anchor, &mut reg);
		let r = graph.refs().next().unwrap();
		let (start, end) = r.position.unwrap();
		assert_eq!(start, 0);
		assert!(end as usize <= source.len());
	}
}
