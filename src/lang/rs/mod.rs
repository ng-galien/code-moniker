//! Rust parser and extractor.

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
	let language: Language = tree_sitter_rust::LANGUAGE.into();
	parser
		.set_language(&language)
		.expect("failed to load tree-sitter Rust grammar");
	parser
		.parse(source, None)
		.expect("tree-sitter parse returned None on a non-cancelled call")
}

pub fn extract(uri: &str, source: &str, anchor: &Moniker) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let mut graph = CodeGraph::new(module.clone(), kinds::MODULE);
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
			.project(b"pg_code_moniker")
			.build()
	}

	#[test]
	fn parse_empty_returns_source_file() {
		let tree = parse("");
		assert_eq!(tree.root_node().kind(), "source_file");
	}

	#[test]
	fn extract_empty_yields_module_only_graph() {
		let anchor = make_anchor();
		let g = extract("src/lib.rs", "", &anchor);
		assert_eq!(g.def_count(), 1);
		assert_eq!(g.ref_count(), 0);

		let expected = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"src")
			.segment(b"module", b"lib")
			.build();
		assert_eq!(g.root(), &expected);
	}

	#[test]
	fn extract_struct_emits_class_def() {
		let g = extract("util.rs", "pub struct Foo { x: i32 }", &make_anchor());
		let foo = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.build();
		assert!(g.contains(&foo));
	}

	#[test]
	fn extract_enum_emits_enum_def() {
		let g = extract("util.rs", "pub enum Color { Red, Green }", &make_anchor());
		let color = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"enum", b"Color")
			.build();
		assert!(g.contains(&color));
	}

	#[test]
	fn extract_trait_emits_interface_def() {
		let g = extract("util.rs", "pub trait Greet { fn hi(&self); }", &make_anchor());
		let greet = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"interface", b"Greet")
			.build();
		assert!(g.contains(&greet));
	}

	#[test]
	fn extract_type_alias_emits_type_alias_def() {
		let g = extract("util.rs", "pub type Id = u64;", &make_anchor());
		let id = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"type_alias", b"Id")
			.build();
		assert!(g.contains(&id));
	}

	#[test]
	fn extract_top_level_fn_emits_function_def() {
		let g = extract("util.rs", "pub fn add(a: i32, b: i32) -> i32 { a + b }", &make_anchor());
		let add = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"add(2)")
			.build();
		assert!(g.contains(&add), "expected {add:?}, defs: {:?}", g.def_monikers());
	}

	#[test]
	fn extract_fn_no_args_uses_arity_zero_form() {
		let g = extract("util.rs", "pub fn boot() {}", &make_anchor());
		let boot = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"boot()")
			.build();
		assert!(g.contains(&boot));
	}

	#[test]
	fn extract_impl_block_reparents_methods_to_type() {
		let src = r#"
            pub struct Foo;
            impl Foo {
                pub fn bar(&self) -> i32 { 0 }
            }
        "#;
		let g = extract("util.rs", src, &make_anchor());
		let foo = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.build();
		let bar = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar(1)")
			.build();
		assert!(g.contains(&foo));
		assert!(g.contains(&bar), "expected {bar:?}, defs: {:?}", g.def_monikers());
	}

	#[test]
	fn extract_impl_trait_for_emits_implements_ref() {
		let src = r#"
            pub trait Greet { fn hi(&self); }
            pub struct Foo;
            impl Greet for Foo {
                fn hi(&self) {}
            }
        "#;
		let g = extract("util.rs", src, &make_anchor());
		let foo = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.build();
		let greet = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"interface", b"Greet")
			.build();
		let r = g.refs().find(|r| r.kind == b"implements".to_vec()).expect("implements ref");
		assert_eq!(g.defs().nth(r.source).unwrap().moniker, foo);
		assert_eq!(r.target, greet);
	}

	#[test]
	fn extract_use_simple_emits_imports_symbol() {
		let g = extract("util.rs", "use foo;", &make_anchor());
		assert_eq!(g.ref_count(), 1);
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"imports_symbol".to_vec());
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"foo")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_scoped_emits_full_path() {
		let g = extract("util.rs", "use std::collections::HashMap;", &make_anchor());
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"std")
			.segment(b"path", b"collections")
			.segment(b"path", b"HashMap")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_list_emits_one_ref_per_leaf() {
		let g = extract("util.rs", "use std::collections::{HashMap, HashSet};", &make_anchor());
		assert_eq!(g.ref_count(), 2);
		let targets: Vec<_> = g.refs().map(|r| r.target.clone()).collect();
		let hashmap = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"std")
			.segment(b"path", b"collections")
			.segment(b"path", b"HashMap")
			.build();
		let hashset = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"std")
			.segment(b"path", b"collections")
			.segment(b"path", b"HashSet")
			.build();
		assert!(targets.contains(&hashmap));
		assert!(targets.contains(&hashset));
	}

	#[test]
	fn extract_use_wildcard_splits_scoped_path() {
		let g = extract("util.rs", "use pgrx::prelude::*;", &make_anchor());
		assert_eq!(g.ref_count(), 1);
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"pgrx")
			.segment(b"path", b"prelude")
			.build();
		assert_eq!(
			g.refs().next().unwrap().target,
			target,
			"wildcard parent path must split on ::, not be captured as one literal"
		);
	}

	#[test]
	fn extract_use_alias_drops_alias_keeps_path() {
		let g = extract("util.rs", "use std::io::Result as IoResult;", &make_anchor());
		assert_eq!(g.ref_count(), 1);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"std")
			.segment(b"path", b"io")
			.segment(b"path", b"Result")
			.build();
		assert_eq!(r.target, target);
	}
}
