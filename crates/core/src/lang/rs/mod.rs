use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use crate::lang::canonical_walker::CanonicalWalker;

pub mod build;
mod canonicalize;
mod kinds;
mod strategy;

use std::collections::HashMap;

use canonicalize::compute_module_moniker;
use strategy::{Strategy, collect_callable_table, collect_local_mods};

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

#[derive(Clone, Debug, Default)]
pub struct Presets {}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let (def_cap, ref_cap) = CodeGraph::capacity_for_source(source.len());
	let mut graph = CodeGraph::with_capacity(module.clone(), kinds::MODULE, def_cap, ref_cap);
	let tree = parse(source);
	let local_mods = collect_local_mods(tree.root_node(), source.as_bytes());
	let mut callable_table: HashMap<(Moniker, Vec<u8>), Vec<u8>> = HashMap::new();
	collect_callable_table(
		tree.root_node(),
		source.as_bytes(),
		&module,
		&mut callable_table,
	);
	let strat = Strategy {
		module: module.clone(),
		source_bytes: source.as_bytes(),
		deep,
		local_mods,
		local_scope: std::cell::RefCell::new(Vec::new()),
		type_params: std::cell::RefCell::new(Vec::new()),
		callable_table,
	};
	let walker = CanonicalWalker::new(&strat, source.as_bytes());
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

pub struct Lang;

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "rs";
	const ALLOWED_KINDS: &'static [&'static str] = &[
		"struct", "enum", "trait", "impl", "fn", "method", "const", "static", "type",
	];
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &["public", "private", "module"];

	fn extract(
		uri: &str,
		source: &str,
		anchor: &Moniker,
		deep: bool,
		presets: &Self::Presets,
	) -> CodeGraph {
		extract(uri, source, anchor, deep, presets)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::moniker::{Moniker, MonikerBuilder};
	use crate::lang::assert_conformance;

	fn extract(uri: &str, source: &str, anchor: &Moniker, deep: bool) -> CodeGraph {
		let g = super::extract(uri, source, anchor, deep, &Presets::default());
		assert_conformance::<super::Lang>(&g, anchor);
		g
	}

	fn make_anchor() -> Moniker {
		MonikerBuilder::new().project(b"code-moniker").build()
	}

	fn has_parent_segment(m: &Moniker, kind: &[u8], name: &[u8]) -> bool {
		let segments: Vec<_> = m.as_view().segments().collect();
		segments
			.get(segments.len().saturating_sub(2))
			.is_some_and(|seg| seg.kind == kind && seg.name == name)
	}

	#[test]
	fn parse_empty_returns_source_file() {
		let tree = parse("");
		assert_eq!(tree.root_node().kind(), "source_file");
	}

	#[test]
	fn extract_collapses_adjacent_line_comments_into_one_def() {
		let src = "// a\n// b\n// c\nstruct Foo;\n";
		let g = extract("src/lib.rs", src, &make_anchor(), false);
		assert_eq!(
			g.defs().filter(|d| d.kind == b"comment").count(),
			1,
			"three adjacent `//` lines collapse to a single comment def"
		);
	}

	#[test]
	fn extract_splits_comments_separated_by_blank_line() {
		let src = "// a\n// b\n\n// c\nstruct Foo;\n";
		let g = extract("src/lib.rs", src, &make_anchor(), false);
		assert_eq!(
			g.defs().filter(|d| d.kind == b"comment").count(),
			2,
			"a blank line breaks the run into two distinct comment defs"
		);
	}

	#[test]
	fn extract_emits_comments_inside_type_bodies() {
		let src = r#"
struct Foo {
    // field comment
    value: i32,
}

trait Bar {
    // trait comment
    fn bar(&self);
}

enum Baz {
    // enum comment
    A,
}
"#;
		let g = extract("src/lib.rs", src, &make_anchor(), true);
		let comments: Vec<_> = g.defs().filter(|d| d.kind == b"comment").collect();
		assert_eq!(comments.len(), 3);
		assert!(
			comments
				.iter()
				.any(|d| has_parent_segment(&d.moniker, b"struct", b"Foo"))
		);
		assert!(
			comments
				.iter()
				.any(|d| has_parent_segment(&d.moniker, b"trait", b"Bar"))
		);
		assert!(
			comments
				.iter()
				.any(|d| has_parent_segment(&d.moniker, b"enum", b"Baz"))
		);
	}

	#[test]
	fn extract_empty_yields_module_only_graph() {
		let anchor = make_anchor();
		let g = extract("src/lib.rs", "", &anchor, false);
		assert_eq!(g.def_count(), 1);
		assert_eq!(g.ref_count(), 0);

		let expected = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"dir", b"src")
			.segment(b"module", b"lib")
			.build();
		assert_eq!(g.root(), &expected);
	}

	#[test]
	fn extract_struct_emits_class_def() {
		let g = extract(
			"util.rs",
			"pub struct Foo { x: i32 }",
			&make_anchor(),
			false,
		);
		let foo = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"struct", b"Foo")
			.build();
		assert!(g.contains(&foo));
	}

	#[test]
	fn extract_enum_emits_enum_def() {
		let g = extract(
			"util.rs",
			"pub enum Color { Red, Green }",
			&make_anchor(),
			false,
		);
		let color = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"enum", b"Color")
			.build();
		assert!(g.contains(&color));
	}

	#[test]
	fn extract_trait_emits_interface_def() {
		let g = extract(
			"util.rs",
			"pub trait Greet { fn hi(&self); }",
			&make_anchor(),
			false,
		);
		let greet = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"trait", b"Greet")
			.build();
		assert!(g.contains(&greet));
	}

	#[test]
	fn extract_type_alias_emits_type_alias_def() {
		let g = extract("util.rs", "pub type Id = u64;", &make_anchor(), false);
		let id = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"type", b"Id")
			.build();
		assert!(g.contains(&id));
	}

	#[test]
	fn extract_top_level_fn_emits_function_def() {
		let g = extract(
			"util.rs",
			"pub fn add(a: i32, b: i32) -> i32 { a + b }",
			&make_anchor(),
			false,
		);
		let add = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"add(a:i32,b:i32)")
			.build();
		assert!(
			g.contains(&add),
			"expected {add:?}, defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_fn_no_args_uses_arity_zero_form() {
		let g = extract("util.rs", "pub fn boot() {}", &make_anchor(), false);
		let boot = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"boot()")
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
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"struct", b"Foo")
			.build();
		let bar = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"struct", b"Foo")
			.segment(b"method", b"bar()")
			.build();
		assert!(g.contains(&foo));
		assert!(
			g.contains(&bar),
			"expected {bar:?}, defs: {:?}",
			g.def_monikers()
		);
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
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"struct", b"Foo")
			.build();
		let greet = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"trait", b"Greet")
			.build();
		let r = g
			.refs()
			.find(|r| r.kind == b"implements".to_vec())
			.expect("implements ref");
		assert_eq!(g.defs().nth(r.source).unwrap().moniker, foo);
		assert_eq!(r.target, greet);
	}

	#[test]
	fn extract_impl_trait_for_external_type_keeps_methods_and_ref() {
		let src = r#"
            use alloc::collections::VecDeque;
            pub trait Buf { fn remaining(&self) -> usize; }
            impl Buf for VecDeque<u8> {
                fn remaining(&self) -> usize { 0 }
                fn chunk(&self) -> &[u8] { &[] }
            }
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		let vec_deque = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"struct", b"VecDeque")
			.build();
		let remaining = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"struct", b"VecDeque")
			.segment(b"method", b"remaining()")
			.build();
		assert!(
			g.contains(&vec_deque),
			"VecDeque must be synthesized as a placeholder struct so its methods can land. defs: {:?}",
			g.def_monikers()
		);
		assert!(
			g.contains(&remaining),
			"method on impl-for-external-type must be captured. defs: {:?}",
			g.def_monikers()
		);
		assert!(
			g.refs().any(|r| r.kind == b"implements".to_vec()
				&& r.target.as_view().segments().last().unwrap().name == b"Buf"),
			"impl-for-external must still emit the implements ref"
		);
	}

	#[test]
	fn extract_use_bare_ident_is_external() {
		let g = extract("util.rs", "use foo;", &make_anchor(), false);
		assert_eq!(g.ref_count(), 1);
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"imports_symbol".to_vec());
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"external_pkg", b"foo")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_external_crate_marks_external_pkg() {
		let g = extract(
			"util.rs",
			"use std::collections::HashMap;",
			&make_anchor(),
			false,
		);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"external_pkg", b"std")
			.segment(b"path", b"collections")
			.segment(b"path", b"HashMap")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_crate_prefix_resolves_project_local() {
		let g = extract(
			"util.rs",
			"use crate::core::moniker::Moniker;",
			&make_anchor(),
			false,
		);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"dir", b"core")
			.segment(b"module", b"moniker")
			.segment(b"path", b"Moniker")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_super_walks_up_one_segment() {
		let anchor = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"path", b"src")
			.segment(b"path", b"lang")
			.build();
		let g = extract("rs/walker.rs", "use super::kinds;", &anchor, false);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"path", b"src")
			.segment(b"path", b"lang")
			.segment(b"lang", b"rs")
			.segment(b"dir", b"rs")
			.segment(b"path", b"kinds")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_local_mod_resolves_as_self() {
		let src = r#"
            mod canonicalize;
            use canonicalize::compute_module_moniker;
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"module", b"canonicalize")
			.segment(b"path", b"compute_module_moniker")
			.build();
		assert_eq!(
			r.target, target,
			"bare path matching a local mod must resolve project-local"
		);
	}

	#[test]
	fn extract_use_unknown_first_segment_stays_external() {
		let g = extract("util.rs", "use foo::bar;", &make_anchor(), false);
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"external_pkg", b"foo")
			.segment(b"path", b"bar")
			.build();
		assert_eq!(g.refs().next().unwrap().target, target);
	}

	#[test]
	fn extract_use_self_keeps_module_prefix() {
		let g = extract("util.rs", "use self::kinds::PATH;", &make_anchor(), false);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"module", b"kinds")
			.segment(b"path", b"PATH")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_use_list_emits_one_ref_per_leaf() {
		let g = extract(
			"util.rs",
			"use std::collections::{HashMap, HashSet};",
			&make_anchor(),
			false,
		);
		let imports_symbol: Vec<_> = g.refs().filter(|r| r.kind == b"imports_symbol").collect();
		assert_eq!(imports_symbol.len(), 2);
		let targets: Vec<_> = imports_symbol.iter().map(|r| r.target.clone()).collect();
		let hashmap = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"external_pkg", b"std")
			.segment(b"path", b"collections")
			.segment(b"path", b"HashMap")
			.build();
		let hashset = MonikerBuilder::new()
			.project(b"code-moniker")
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
		let imports_symbol: Vec<_> = g.refs().filter(|r| r.kind == b"imports_symbol").collect();
		assert_eq!(imports_symbol.len(), 1);
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"external_pkg", b"pgrx")
			.segment(b"path", b"prelude")
			.build();
		assert_eq!(
			imports_symbol[0].target, target,
			"wildcard parent path must split on :: AND mark crate root as external"
		);
	}

	#[test]
	fn extract_use_alias_drops_alias_keeps_path() {
		let g = extract(
			"util.rs",
			"use std::io::Result as IoResult;",
			&make_anchor(),
			false,
		);
		let imports_symbol: Vec<_> = g.refs().filter(|r| r.kind == b"imports_symbol").collect();
		assert_eq!(imports_symbol.len(), 1);
		let r = &imports_symbol[0];
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"external_pkg", b"std")
			.segment(b"path", b"io")
			.segment(b"path", b"Result")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_shallow_skips_param_and_local() {
		let src = "pub fn add(a: i32, b: i32) -> i32 { let sum = a + b; sum }";
		let g = extract("util.rs", src, &make_anchor(), false);
		assert!(
			g.defs().all(|d| d.kind != b"param" && d.kind != b"local"),
			"shallow extraction must not produce param/local defs"
		);
	}

	#[test]
	fn extract_deep_emits_params_under_function() {
		let src = "pub fn add(a: i32, b: i32) -> i32 { a + b }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let add = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"add(a:i32,b:i32)")
			.build();
		let pa = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"add(a:i32,b:i32)")
			.segment(b"param", b"a")
			.build();
		let pb = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"add(a:i32,b:i32)")
			.segment(b"param", b"b")
			.build();
		assert!(g.contains(&add));
		assert!(
			g.contains(&pa),
			"missing param:a, defs: {:?}",
			g.def_monikers()
		);
		assert!(g.contains(&pb));
	}

	#[test]
	fn extract_deep_self_parameter_named_self() {
		let src = "pub struct Foo; impl Foo { fn bar(&self, x: i32) {} }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let bar_self = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"struct", b"Foo")
			.segment(b"method", b"bar(x:i32)")
			.segment(b"param", b"self")
			.build();
		let bar_x = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"struct", b"Foo")
			.segment(b"method", b"bar(x:i32)")
			.segment(b"param", b"x")
			.build();
		assert!(g.contains(&bar_self));
		assert!(g.contains(&bar_x));
	}

	#[test]
	fn extract_deep_emits_locals_under_function() {
		let src = r#"pub fn run() {
            let x = 1;
            let y = 2;
        }"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let lx = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"run()")
			.segment(b"local", b"x")
			.build();
		let ly = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"run()")
			.segment(b"local", b"y")
			.build();
		assert!(g.contains(&lx));
		assert!(g.contains(&ly));
	}

	#[test]
	fn extract_deep_locals_in_nested_block_attach_to_function() {
		let src = r#"pub fn run(flag: bool) {
            if flag { let inner = 1; }
        }"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let inner = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"run(flag:bool)")
			.segment(b"local", b"inner")
			.build();
		assert!(
			g.contains(&inner),
			"local inside `if` block should attach to the function, not the block; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_deep_named_closure_emits_function_def() {
		let src = "pub fn run() { let f = |x| x + 1; }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let f = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"run()")
			.segment(b"fn", b"f(x)")
			.build();
		assert!(
			g.contains(&f),
			"expected {f:?}, defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_deep_skips_underscore_pattern() {
		let src = "pub fn run(_: i32) { let _ = 1; }";
		let g = extract("util.rs", src, &make_anchor(), true);
		assert!(
			g.defs().all(|d| d.kind != b"param" && d.kind != b"local"),
			"`_` patterns must not produce defs; got: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_deep_comment_inside_match_arm_emits_def() {
		let src = r#"pub fn run() {
            match Some(1) {
                Some(_) => {}
                // inside-arm
                None => {}
            }
        }"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		assert!(
			g.defs().any(|d| d.kind == b"comment"),
			"comment between match arms must emit a comment def; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_deep_comment_inside_let_value_expression_emits_def() {
		let src = r#"pub fn run() {
            let _value = if true {
                1
            } else {
                match Some(2) {
                    Some(_) => 3,
                    // hidden-in-let-value
                    None => 4,
                }
            };
        }"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		assert!(
			g.defs().any(|d| d.kind == b"comment"),
			"comment nested in the value of a let must emit a comment def; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_deep_comment_inside_call_closure_emits_def() {
		let src = r#"pub fn run() {
            let _ = (0..1).map(|x| {
                // hidden-in-closure-arg
                x + 1
            });
        }"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		assert!(
			g.defs().any(|d| d.kind == b"comment"),
			"comment inside a closure passed as a call argument must emit a comment def; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_deep_local_inside_let_value_emits_def() {
		let src = r#"pub fn run() {
            let _v = if true {
                let inner = 7;
                inner
            } else {
                0
            };
        }"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let inner = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"fn", b"run()")
			.segment(b"local", b"inner")
			.build();
		assert!(
			g.contains(&inner),
			"local inside a let-value expression must attach to the function; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_self_dot_method_emits_method_call_ref() {
		let src = r#"
pub struct W;
impl W {
    fn dispatch(&self) { self.walk(); }
    fn walk(&self) {}
}
"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let refs: Vec<_> = g.refs().filter(|r| r.kind == b"method_call").collect();
		assert_eq!(
			refs.len(),
			1,
			"expected one method_call ref; refs: {:?}",
			refs
		);
		let target = &refs[0].target;
		let last = target.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"method");
		let bare = crate::core::moniker::query::bare_callable_name(last.name);
		assert_eq!(
			bare,
			b"walk",
			"method_call target must point at `walk`; got name={:?}",
			std::str::from_utf8(last.name)
		);
		let source_def = g.def_at(refs[0].source);
		let source_last = source_def.moniker.as_view().segments().last().unwrap();
		let source_bare = crate::core::moniker::query::bare_callable_name(source_last.name);
		assert_eq!(
			source_bare,
			b"dispatch",
			"method_call source must be `dispatch`; got name={:?}",
			std::str::from_utf8(source_last.name)
		);
	}

	#[test]
	fn extract_non_self_method_call_emits_method_call_ref() {
		let src = r#"
pub struct W;
impl W {
    fn run(&self, other: W) { other.walk(); }
    fn walk(&self) {}
}
"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g.refs().filter(|r| r.kind == b"method_call").count();
		assert!(
			n >= 1,
			"non-self receiver must emit method_call with arity-only target; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_nested_self_call_emits_two_method_call_refs() {
		let src = r#"
pub struct W;
impl W {
    fn outer(&self) { self.foo(self.bar()); }
    fn foo(&self, _: u8) {}
    fn bar(&self) -> u8 { 0 }
}
"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g.refs().filter(|r| r.kind == b"method_call").count();
		assert_eq!(
			n,
			2,
			"nested self.foo(self.bar()) must emit two method_call refs; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_use_emits_imports_module_to_parent() {
		let src = "use crate::foo::bar::Baz;";
		let g = extract("lib.rs", src, &make_anchor(), false);
		let ims: Vec<_> = g.refs().filter(|r| r.kind == b"imports_module").collect();
		assert!(
			!ims.is_empty(),
			"use must emit imports_module; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
		let last = ims[0].target.as_view().segments().last().unwrap();
		assert_ne!(
			last.kind,
			b"path",
			"imports_module target must point at a module, not at the leaf path:Baz; last={:?}",
			std::str::from_utf8(last.kind)
		);
	}

	#[test]
	fn extract_use_external_emits_imports_module() {
		let src = "use std::collections::HashMap;";
		let g = extract("lib.rs", src, &make_anchor(), false);
		let n = g.refs().filter(|r| r.kind == b"imports_module").count();
		assert!(
			n >= 1,
			"extern use must emit imports_module; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_use_single_segment_skips_imports_module() {
		let src = "use foo;";
		let g = extract("lib.rs", src, &make_anchor(), false);
		let n = g.refs().filter(|r| r.kind == b"imports_module").count();
		assert_eq!(n, 0, "single-segment use has no parent module to point at");
	}

	#[test]
	fn extract_grouped_use_emits_single_imports_module_per_parent() {
		let src = "use std::io::{self, Read, Write};";
		let g = extract("lib.rs", src, &make_anchor(), false);
		let ims: Vec<_> = g.refs().filter(|r| r.kind == b"imports_module").collect();
		assert_eq!(
			ims.len(),
			1,
			"grouped use must emit exactly one imports_module per parent module; refs: {:?}",
			ims
		);
	}

	#[test]
	fn extract_nested_grouped_use_emits_one_imports_module_per_distinct_parent() {
		let src = "use std::{io::{Read, Write}, fmt};";
		let g = extract("lib.rs", src, &make_anchor(), false);
		let all: Vec<_> = g.refs().filter(|r| r.kind == b"imports_module").collect();
		let unique: std::collections::HashSet<_> = all.iter().map(|r| &r.target).collect();
		assert_eq!(
			all.len(),
			unique.len(),
			"nested grouped use must not duplicate imports_module refs for the same parent",
		);
	}

	#[test]
	fn extract_free_function_call_emits_calls_ref() {
		let src = "pub fn run() { foo(); }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g.refs().filter(|r| r.kind == b"calls").count();
		assert!(
			n >= 1,
			"free fn call must emit calls ref; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_path_qualified_call_emits_calls_ref() {
		let src = "pub fn run() { ::foo::bar::baz(); }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g.refs().filter(|r| r.kind == b"calls").count();
		assert!(
			n >= 1,
			"path-qualified call must emit calls ref; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_param_type_emits_uses_type_ref() {
		let src = "pub fn run(x: SomeType) {}";
		let g = extract("util.rs", src, &make_anchor(), false);
		let n = g.refs().filter(|r| r.kind == b"uses_type").count();
		assert!(
			n >= 1,
			"param type annotation must emit uses_type; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_return_type_emits_uses_type_ref() {
		let src = "pub fn run() -> SomeType { todo!() }";
		let g = extract("util.rs", src, &make_anchor(), false);
		let n = g.refs().filter(|r| r.kind == b"uses_type").count();
		assert!(
			n >= 1,
			"return type must emit uses_type; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_let_type_emits_uses_type_ref() {
		let src = "pub fn run() { let x: SomeType = todo!(); }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g.refs().filter(|r| r.kind == b"uses_type").count();
		assert!(
			n >= 1,
			"typed let binding must emit uses_type; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_struct_field_type_emits_uses_type_ref() {
		let src = "pub struct Foo { pub value: SomeType }";
		let g = extract("util.rs", src, &make_anchor(), false);
		let n = g.refs().filter(|r| r.kind == b"uses_type").count();
		assert!(
			n >= 1,
			"struct field type must emit uses_type; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_struct_literal_emits_instantiates_ref() {
		let src = "pub fn run() { let _ = Foo { x: 1 }; }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g.refs().filter(|r| r.kind == b"instantiates").count();
		assert!(
			n >= 1,
			"struct literal must emit instantiates; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_path_constructor_emits_instantiates_ref() {
		let src = "pub fn run() { let _ = Foo::new(); }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g.refs().filter(|r| r.kind == b"instantiates").count();
		assert!(
			n >= 1,
			"Foo::new() must emit instantiates; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_comprehensive_fixture_covers_all_expected_ref_kinds() {
		let src = r#"
use std::collections::HashMap;
use crate::foo::Bar;

pub trait Greet { fn hi(&self); }

pub struct Service { backing: HashMap<String, Bar> }

impl Greet for Service {
    fn hi(&self) {
        let _ = Service { backing: HashMap::new() };
        let other: Service = Service::new();
        other.hi();
        self.hi();
        helper();
    }
}

pub fn helper() {}
"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let kinds: std::collections::HashSet<Vec<u8>> = g.refs().map(|r| r.kind.clone()).collect();
		let expected: &[&[u8]] = &[
			b"imports_module",
			b"imports_symbol",
			b"calls",
			b"method_call",
			b"uses_type",
			b"instantiates",
			b"implements",
		];
		let missing: Vec<&str> = expected
			.iter()
			.filter(|k| !kinds.contains(*k as &[u8]))
			.map(|k| std::str::from_utf8(k).unwrap())
			.collect();
		assert!(
			missing.is_empty(),
			"missing ref kinds in comprehensive fixture: {:?}; got: {:?}",
			missing,
			kinds
				.iter()
				.map(|k| std::str::from_utf8(k).unwrap_or("?"))
				.collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_method_chain_emits_method_call_per_link() {
		let src = r#"
pub struct W;
impl W {
    fn outer(&self) { self.foo().bar(); }
    fn foo(&self) -> Self { W }
    fn bar(&self) {}
}
"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g.refs().filter(|r| r.kind == b"method_call").count();
		assert_eq!(
			n,
			2,
			"method chain self.foo().bar() must emit one method_call per link; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_enum_variant_construction_emits_instantiates() {
		let src = r#"
pub fn run() { let _ = Color::Red(1); }
"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g
			.refs()
			.filter(|r| {
				r.kind == b"instantiates"
					&& r.target
						.as_view()
						.segments()
						.last()
						.is_some_and(|s| s.kind == b"enum" && s.name == b"Color")
			})
			.count();
		assert_eq!(
			n,
			1,
			"Type::Variant(args) must emit instantiates → enum:Type; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_tuple_struct_construction_emits_instantiates() {
		let src = "pub fn run() { let _ = Foo(1, 2); }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g
			.refs()
			.filter(|r| {
				r.kind == b"instantiates"
					&& r.target
						.as_view()
						.segments()
						.last()
						.is_some_and(|s| s.kind == b"struct" && s.name == b"Foo")
			})
			.count();
		assert_eq!(
			n,
			1,
			"CamelCase identifier call Foo(...) must emit instantiates → struct:Foo; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
		let mistaken = g
			.refs()
			.filter(|r| r.kind == b"calls")
			.any(|r| r.target.as_view().segments().last().unwrap().name == b"Foo(2)");
		assert!(
			!mistaken,
			"Foo(...) must NOT emit calls → fn:Foo; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_macro_invocation_emits_calls_ref() {
		let src = "pub fn run() { vec![1, 2]; format!(\"{}\", 1); }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let names: Vec<_> = g
			.refs()
			.filter(|r| r.kind == b"calls")
			.map(|r| {
				r.target
					.as_view()
					.segments()
					.last()
					.map(|s| s.name.to_vec())
					.unwrap_or_default()
			})
			.collect();
		assert!(
			names.iter().any(|n| n.starts_with(b"vec")),
			"vec! must emit calls; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
		assert!(
			names.iter().any(|n| n.starts_with(b"format")),
			"format! must emit calls; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_primitive_types_emit_no_uses_type_ref() {
		let src = "pub fn run(x: i32, y: bool, z: String) -> u8 { let _: f64 = 0.0; 0 }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let primitives: &[&[u8]] = &[b"i32", b"u8", b"bool", b"f64", b"String", b"str"];
		let leaked: Vec<_> = g
			.refs()
			.filter(|r| r.kind == b"uses_type")
			.filter(|r| {
				let name = r.target.as_view().segments().last().unwrap().name;
				primitives.contains(&name)
			})
			.collect();
		assert!(
			leaked.is_empty(),
			"primitive types must NOT emit uses_type; leaked: {:?}",
			leaked
		);
	}

	#[test]
	fn extract_generic_type_param_emits_no_uses_type_ref() {
		let src = "pub fn run<T>(x: T) -> T { x }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let leaked = g
			.refs()
			.filter(|r| r.kind == b"uses_type")
			.any(|r| r.target.as_view().segments().last().unwrap().name == b"T");
		assert!(
			!leaked,
			"generic type param T must NOT emit uses_type; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_closure_bound_call_targets_local_def() {
		let src = "pub fn run() { let f = |x| x + 1; f(2); }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let local_call = g
			.refs()
			.filter(|r| r.kind == b"calls")
			.find(|r| r.target.as_view().segments().last().unwrap().name == b"f");
		assert!(
			local_call.is_some(),
			"call to closure-bound name `f` must target the local closure def; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
		let r = local_call.unwrap();
		assert_eq!(
			r.confidence,
			b"local",
			"closure-bound call confidence must be `local`, got {:?}",
			std::str::from_utf8(&r.confidence)
		);
	}

	#[test]
	fn extract_scoped_variant_in_value_position_emits_reads() {
		let src = "pub fn run() { let _ = Color::Red; }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let n = g
			.refs()
			.filter(|r| r.kind == b"reads")
			.any(|r| r.target.as_view().segments().last().unwrap().name == b"Red");
		assert!(
			n,
			"Color::Red in value position must emit reads → variant; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_trait_supertype_emits_extends_ref() {
		let src = "pub trait Foo: Bar + Baz {}";
		let g = extract("util.rs", src, &make_anchor(), false);
		let extends: Vec<_> = g.refs().filter(|r| r.kind == b"extends").collect();
		assert_eq!(
			extends.len(),
			2,
			"trait Foo: Bar + Baz must emit two extends refs; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_derive_attribute_emits_annotates_ref() {
		let src = "#[derive(Clone, Debug)] pub struct Foo;";
		let g = extract("util.rs", src, &make_anchor(), false);
		let n = g.refs().filter(|r| r.kind == b"annotates").count();
		assert!(
			n >= 2,
			"#[derive(Clone, Debug)] must emit at least 2 annotates refs (one per trait); refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_local_var_reference_emits_reads_ref() {
		let src = "pub fn run() { let x = 1; foo(x); }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let reads_x = g
			.refs()
			.filter(|r| r.kind == b"reads")
			.any(|r| r.target.as_view().segments().last().unwrap().name == b"x");
		assert!(
			reads_x,
			"local variable read `x` must emit reads → local:x; refs: {:?}",
			g.refs().collect::<Vec<_>>()
		);
	}
}
