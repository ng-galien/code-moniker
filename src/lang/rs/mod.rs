//! Rust parser and extractor.

use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

pub mod build;
mod canonicalize;
mod kinds;
mod refs;
mod walker;

use canonicalize::compute_module_moniker;
use walker::{collect_local_mods, Walker};

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

pub fn extract(uri: &str, source: &str, anchor: &Moniker, deep: bool) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let mut graph = CodeGraph::new(module.clone(), kinds::MODULE);
	let tree = parse(source);
	let local_mods = collect_local_mods(tree.root_node(), source.as_bytes());
	let walker = Walker {
		source_bytes: source.as_bytes(),
		module: module.clone(),
		local_mods,
		deep,
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
		let g = extract("src/lib.rs", "", &anchor, false);
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
		let g = extract("util.rs", "pub struct Foo { x: i32 }", &make_anchor(), false);
		let foo = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.build();
		assert!(g.contains(&foo));
	}

	#[test]
	fn extract_enum_emits_enum_def() {
		let g = extract("util.rs", "pub enum Color { Red, Green }", &make_anchor(), false);
		let color = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"enum", b"Color")
			.build();
		assert!(g.contains(&color));
	}

	#[test]
	fn extract_trait_emits_interface_def() {
		let g = extract("util.rs", "pub trait Greet { fn hi(&self); }", &make_anchor(), false);
		let greet = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"interface", b"Greet")
			.build();
		assert!(g.contains(&greet));
	}

	#[test]
	fn extract_type_alias_emits_type_alias_def() {
		let g = extract("util.rs", "pub type Id = u64;", &make_anchor(), false);
		let id = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"type_alias", b"Id")
			.build();
		assert!(g.contains(&id));
	}

	#[test]
	fn extract_top_level_fn_emits_function_def() {
		let g = extract("util.rs", "pub fn add(a: i32, b: i32) -> i32 { a + b }", &make_anchor(), false);
		let add = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"add(2)")
			.build();
		assert!(g.contains(&add), "expected {add:?}, defs: {:?}", g.def_monikers());
	}

	#[test]
	fn extract_fn_no_args_uses_arity_zero_form() {
		let g = extract("util.rs", "pub fn boot() {}", &make_anchor(), false);
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
		let g = extract("util.rs", src, &make_anchor(), false);
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
		let g = extract("util.rs", src, &make_anchor(), false);
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
	fn extract_use_bare_ident_is_external() {
		let g = extract("util.rs", "use foo;", &make_anchor(), false);
		assert_eq!(g.ref_count(), 1);
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"imports_symbol".to_vec());
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"external_pkg", b"foo")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_external_crate_marks_external_pkg() {
		let g = extract("util.rs", "use std::collections::HashMap;", &make_anchor(), false);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"external_pkg", b"std")
			.segment(b"path", b"collections")
			.segment(b"path", b"HashMap")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_crate_prefix_resolves_project_local() {
		let g = extract("util.rs", "use crate::core::moniker::Moniker;", &make_anchor(), false);
		let r = g.refs().next().unwrap();
		// `crate::` prefix stripped; rest encoded as project-local path.
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"core")
			.segment(b"path", b"moniker")
			.segment(b"path", b"Moniker")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_super_walks_up_one_segment() {
		// Module is anchor + `module:rs` + `module:walker` (we stub a 2-deep file).
		let anchor = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"src")
			.segment(b"path", b"lang")
			.build();
		let g = extract("rs/walker.rs", "use super::kinds;", &anchor, false);
		let r = g.refs().next().unwrap();
		// `super::` strips one segment from the importer's module
		// (`module:walker`), then appends `path:kinds`.
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"path", b"src")
			.segment(b"path", b"lang")
			.segment(b"path", b"rs")
			.segment(b"path", b"kinds")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_local_mod_resolves_as_self() {
		// `mod canonicalize;` declared at file root → bare
		// `use canonicalize::X;` is project-local, not external.
		let src = r#"
            mod canonicalize;
            use canonicalize::compute_module_moniker;
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"path", b"canonicalize")
			.segment(b"path", b"compute_module_moniker")
			.build();
		assert_eq!(
			r.target, target,
			"bare path matching a local mod must resolve project-local"
		);
	}

	#[test]
	fn extract_use_unknown_first_segment_stays_external() {
		// No `mod foo;` at root → `foo::bar` is treated as external.
		let g = extract("util.rs", "use foo::bar;", &make_anchor(), false);
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"external_pkg", b"foo")
			.segment(b"path", b"bar")
			.build();
		assert_eq!(g.refs().next().unwrap().target, target);
	}

	#[test]
	fn extract_use_self_keeps_module_prefix() {
		let g = extract("util.rs", "use self::kinds::PATH;", &make_anchor(), false);
		let r = g.refs().next().unwrap();
		// `self::` resolves under the importer's module moniker.
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"path", b"kinds")
			.segment(b"path", b"PATH")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_list_emits_one_ref_per_leaf() {
		let g = extract("util.rs", "use std::collections::{HashMap, HashSet};", &make_anchor(), false);
		assert_eq!(g.ref_count(), 2);
		let targets: Vec<_> = g.refs().map(|r| r.target.clone()).collect();
		let hashmap = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"external_pkg", b"std")
			.segment(b"path", b"collections")
			.segment(b"path", b"HashMap")
			.build();
		let hashset = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"external_pkg", b"std")
			.segment(b"path", b"collections")
			.segment(b"path", b"HashSet")
			.build();
		assert!(targets.contains(&hashmap));
		assert!(targets.contains(&hashset));
	}

	#[test]
	fn extract_use_wildcard_splits_scoped_path() {
		let g = extract("util.rs", "use pgrx::prelude::*;", &make_anchor(), false);
		assert_eq!(g.ref_count(), 1);
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"external_pkg", b"pgrx")
			.segment(b"path", b"prelude")
			.build();
		assert_eq!(
			g.refs().next().unwrap().target,
			target,
			"wildcard parent path must split on :: AND mark crate root as external"
		);
	}

	#[test]
	fn extract_use_alias_drops_alias_keeps_path() {
		let g = extract("util.rs", "use std::io::Result as IoResult;", &make_anchor(), false);
		assert_eq!(g.ref_count(), 1);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"external_pkg", b"std")
			.segment(b"path", b"io")
			.segment(b"path", b"Result")
			.build();
		assert_eq!(r.target, target);
	}

	// --- deep extraction (deep=true) -------------------------------------

	#[test]
	fn deep_off_emits_no_param_or_local_defs() {
		let src = "pub fn add(a: i32, b: i32) -> i32 { let sum = a + b; sum }";
		let g = extract("util.rs", src, &make_anchor(), false);
		assert!(
			g.defs().all(|d| d.kind != b"param" && d.kind != b"local"),
			"shallow extraction must not produce param/local defs"
		);
	}

	#[test]
	fn deep_emits_params_under_function() {
		let src = "pub fn add(a: i32, b: i32) -> i32 { a + b }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let add = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"add(2)")
			.build();
		let pa = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"add(2)")
			.segment(b"param", b"a")
			.build();
		let pb = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"add(2)")
			.segment(b"param", b"b")
			.build();
		assert!(g.contains(&add));
		assert!(g.contains(&pa), "missing param:a, defs: {:?}", g.def_monikers());
		assert!(g.contains(&pb));
	}

	#[test]
	fn deep_self_parameter_named_self() {
		let src = "pub struct Foo; impl Foo { fn bar(&self, x: i32) {} }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let bar_self = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar(2)")
			.segment(b"param", b"self")
			.build();
		let bar_x = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar(2)")
			.segment(b"param", b"x")
			.build();
		assert!(g.contains(&bar_self));
		assert!(g.contains(&bar_x));
	}

	#[test]
	fn deep_emits_locals_under_enclosing_function() {
		let src = r#"pub fn run() {
            let x = 1;
            let y = 2;
        }"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let lx = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"run()")
			.segment(b"local", b"x")
			.build();
		let ly = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"run()")
			.segment(b"local", b"y")
			.build();
		assert!(g.contains(&lx));
		assert!(g.contains(&ly));
	}

	#[test]
	fn deep_locals_in_nested_block_attach_to_function() {
		// Containment rule: a `let` inside an `if { }` is parented to
		// the enclosing function, not to the block.
		let src = r#"pub fn run(flag: bool) {
            if flag { let inner = 1; }
        }"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let inner = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"run(1)")
			.segment(b"local", b"inner")
			.build();
		assert!(
			g.contains(&inner),
			"local inside `if` block should attach to the function, not the block; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn deep_named_closure_emits_function_def_under_callable() {
		let src = "pub fn run() { let f = |x| x + 1; }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let f = MonikerBuilder::new()
			.project(b"pg_code_moniker")
			.segment(b"module", b"util")
			.segment(b"function", b"run()")
			.segment(b"function", b"f(1)")
			.build();
		assert!(g.contains(&f), "expected {f:?}, defs: {:?}", g.def_monikers());
	}

	#[test]
	fn deep_skips_underscore_pattern() {
		let src = "pub fn run(_: i32) { let _ = 1; }";
		let g = extract("util.rs", src, &make_anchor(), true);
		assert!(
			g.defs().all(|d| d.kind != b"param" && d.kind != b"local"),
			"`_` patterns must not produce defs; got: {:?}",
			g.def_monikers()
		);
	}
}
