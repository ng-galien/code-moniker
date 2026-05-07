//! TypeScript parser and extractor.

use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

mod canonicalize;
mod kinds;
mod refs;
mod walker;

use canonicalize::compute_module_moniker;
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

pub fn extract(uri: &str, source: &str, anchor: &Moniker) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri, kinds::PATH);
	let mut graph = CodeGraph::new(module.clone(), kinds::PATH);
	let tree = parse(source);
	let walker = Walker {
		source_bytes: source.as_bytes(),
		module: module.clone(),
	};
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::moniker::MonikerBuilder;

	fn make_anchor() -> Moniker {
		MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.build()
	}

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

	#[test]
	fn extract_empty_source_yields_module_only_graph() {
		let anchor = make_anchor();
		let graph = extract("src/lib/util.ts", "", &anchor);
		assert_eq!(graph.def_count(), 1);
		assert_eq!(graph.ref_count(), 0);

		let expected = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"path", b"src")
			.segment(b"path", b"lib")
			.segment(b"path", b"util")
			.build();
		assert_eq!(graph.root(), &expected);
	}

	#[test]
	fn extract_strips_each_known_extension() {
		let anchor = make_anchor();
		for uri in ["foo.ts", "foo.tsx", "foo.js", "foo.jsx", "foo.mjs", "foo.cjs"] {
			let g = extract(uri, "", &anchor);
			let last = g.root().as_view().segments().last().unwrap();
			assert_eq!(last.name, b"foo", "extension not stripped on {uri}");
		}
	}

	#[test]
	fn extract_simple_class_emits_class_def() {
		let anchor = make_anchor();
		let graph = extract("util.ts", "class Foo {}", &anchor);
		assert_eq!(graph.def_count(), 2);

		let foo = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"path", b"util")
			.segment(b"class", b"Foo")
			.build();
		assert!(graph.contains(&foo));
	}

	#[test]
	fn extract_export_class_descends_into_export_statement() {
		let anchor = make_anchor();
		let graph = extract("util.ts", "export class Foo {}", &anchor);
		assert_eq!(graph.def_count(), 2);
	}

	#[test]
	fn extract_class_with_method_emits_method_def() {
		let anchor = make_anchor();
		let graph = extract("util.ts", "class Foo { bar() {} }", &anchor);
		assert_eq!(graph.def_count(), 3);

		let bar = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"path", b"util")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar()")
			.build();
		assert!(graph.contains(&bar));
	}

	#[test]
	fn extract_function_declaration_emits_def() {
		let anchor = make_anchor();
		let graph = extract("util.ts", "function foo() {}", &anchor);
		assert_eq!(graph.def_count(), 2);

		let foo = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"path", b"util")
			.segment(b"function", b"foo()")
			.build();
		assert!(graph.contains(&foo));
	}

	#[test]
	fn extract_import_resolves_relative_path_against_importer_dir() {
		let anchor = make_anchor();
		let graph = extract(
			"src/util.ts",
			"import { Bar } from './bar';",
			&anchor,
		);
		assert_eq!(graph.ref_count(), 1);

		let r = graph.refs().next().unwrap();
		assert_eq!(r.kind, b"import".to_vec(), "ref carries the semantic 'import' kind");
		assert_eq!(r.source, 0, "ref attached to the module root");

		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"path", b"src")
			.segment(b"path", b"bar")
			.build();
		assert_eq!(r.target, target, "import resolved against importer dir");
	}

	#[test]
	fn extract_import_walks_up_with_dotdot() {
		let anchor = make_anchor();
		let graph = extract(
			"src/lib/foo.ts",
			"import { X } from '../other';",
			&anchor,
		);
		let r = graph.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"path", b"src")
			.segment(b"path", b"other")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_import_position_covers_statement() {
		let anchor = make_anchor();
		let source = "import { Bar } from './bar';";
		let graph = extract("util.ts", source, &anchor);
		let r = graph.refs().next().unwrap();
		let (start, end) = r.position.unwrap();
		assert_eq!(start, 0);
		assert!(end as usize <= source.len());
	}
}
