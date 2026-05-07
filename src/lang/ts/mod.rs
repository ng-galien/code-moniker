//! TypeScript parser and extractor.

use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::kind_registry::KindRegistry;
use crate::core::moniker::Moniker;

mod canonicalize;
mod kinds;
mod refs;
mod walker;

use canonicalize::compute_module_moniker;
use kinds::TsKinds;
use walker::Walker;

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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::kind_registry::{KindId, PunctClass};
	use crate::core::moniker::MonikerBuilder;

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
	fn extract_import_resolves_relative_path_against_importer_dir() {
		let (mut reg, anchor, path) = make_anchor();
		let graph = extract(
			"src/util.ts",
			"import { Bar } from './bar';",
			&anchor,
			&mut reg,
		);
		assert_eq!(graph.ref_count(), 1);

		let r = graph.refs().next().unwrap();
		let import_kid = reg.intern("import", PunctClass::Path).unwrap();
		assert_eq!(r.kind, import_kid, "ref carries the semantic 'import' kind");
		assert_eq!(r.source, 0, "ref attached to the module root");

		// Importer is at my-app/main/src/util; "./bar" → my-app/main/src/bar.
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.segment(path, b"src")
			.segment(path, b"bar")
			.build();
		assert_eq!(r.target, target, "import resolved against importer dir");
	}

	#[test]
	fn extract_import_walks_up_with_dotdot() {
		let (mut reg, anchor, path) = make_anchor();
		let graph = extract(
			"src/lib/foo.ts",
			"import { X } from '../other';",
			&anchor,
			&mut reg,
		);
		let r = graph.refs().next().unwrap();
		// Importer is my-app/main/src/lib/foo; "../other" → my-app/main/src/other.
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.segment(path, b"src")
			.segment(path, b"other")
			.build();
		assert_eq!(r.target, target);
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
