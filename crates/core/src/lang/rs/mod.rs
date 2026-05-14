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
		in_trait_impl: std::cell::Cell::new(false),
		imported_modules: std::cell::RefCell::new(std::collections::HashSet::new()),
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
		"struct", "enum", "trait", "impl", "fn", "method", "test", "const", "static", "type",
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
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"dir", b"src")
			.segment(b"module", b"lib")
			.build();
		assert_eq!(g.root(), &expected);
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
	fn extract_rust_test_function_emits_test_def_with_metadata() {
		let src = r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn parses_order() {}
            }
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		let test = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"module", b"tests")
			.segment(b"test", b"parses_order()")
			.build();
		let def = g
			.defs()
			.find(|d| d.moniker == test)
			.expect("expected #[test] function to be represented as a test def");
		assert_eq!(def.kind, b"test".to_vec());
		assert_eq!(
			def.signature,
			b"framework=rust-test;enabled=true;display=parses_order".to_vec()
		);
	}

	#[test]
	fn extract_ignored_rust_test_marks_test_disabled() {
		let src = r#"
            #[test]
            #[ignore = "requires external service"]
            fn skipped() {}
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		let test = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"lang", b"rs")
			.segment(b"module", b"util")
			.segment(b"test", b"skipped()")
			.build();
		let def = g.defs().find(|d| d.moniker == test).unwrap();
		assert_eq!(
			def.signature,
			b"framework=rust-test;enabled=false;display=skipped;ignore=requires external service"
				.to_vec()
		);
	}

	#[test]
	fn extract_scoped_test_attribute_is_not_builtin_rust_test() {
		let src = r#"
            #[tokio::test]
            async fn async_runtime_test() {}
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		assert!(
			!g.defs().any(|d| d.kind == b"test"),
			"scoped test proc macro attributes should not be classified as built-in rust-test defs"
		);
	}

	#[test]
	fn extract_proptest_function_emits_test_def_with_proptest_framework() {
		let src = r#"
            proptest::proptest! {
                #[test]
                fn round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..16)) {
                    let _ = bytes;
                }
            }
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		assert!(
			g.defs().any(|d| {
				d.kind == b"test".to_vec()
					&& d.signature
						== b"framework=proptest;enabled=true;display=round_trips".to_vec()
					&& d.moniker.as_view().segments().last().unwrap().name == b"round_trips(bytes)"
			}),
			"proptest #[test] should appear as a test def. defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_ignored_proptest_function_marks_test_disabled() {
		let src = r#"
            proptest::proptest! {
                #[ignore = "slow property"]
                #[test]
                fn ignored_property(value in 0usize..8) {}
            }
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		assert!(
			g.defs().any(|d| {
				d.kind == b"test".to_vec()
					&& d.signature
						== b"framework=proptest;enabled=false;display=ignored_property;ignore=slow property"
							.to_vec()
					&& d.moniker.as_view().segments().last().unwrap().name
						== b"ignored_property(value)"
			}),
			"ignored proptest #[test] should appear disabled. defs: {:?}",
			g.def_monikers()
		);
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
	fn extract_builtin_macro_marks_external_std_reference() {
		let g = extract(
			"util.rs",
			"fn demo() { let xs = vec![1, 2, 3]; }",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls" && r.target.as_view().segments().last().unwrap().name == b"vec"
			})
			.expect("vec! macro call should be represented");
		assert_eq!(r.confidence, b"external".to_vec());
		let target = MonikerBuilder::new()
			.project(b"code-moniker")
			.segment(b"external_pkg", b"std")
			.segment(b"path", b"macros")
			.segment(b"macro", b"vec")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_common_iterator_chain_methods_are_external_when_receiver_is_a_call() {
		let src = r#"
            fn demo(values: Vec<i32>) -> Vec<i32> {
                let mut out = Vec::new();
                out.push(1);
                values.into_iter().map(|v| v + 1).collect()
            }
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		for name in ["map", "collect"] {
			let r = g
				.refs()
				.find(|r| {
					r.kind == b"method_call"
						&& r.target.as_view().segments().last().unwrap().name == name.as_bytes()
				})
				.unwrap_or_else(|| panic!("missing method call {name}"));
			assert_eq!(
				r.confidence,
				b"external".to_vec(),
				"{name} should not be surfaced as an unresolved project method"
			);
			let mut segs = r.target.as_view().segments();
			let head = segs.next().expect("external target has head");
			assert_eq!(head.kind, b"external_pkg");
			assert_eq!(head.name, b"std");
		}
		for name in ["push", "into_iter"] {
			let r = g
				.refs()
				.find(|r| {
					r.kind == b"method_call"
						&& r.target.as_view().segments().last().unwrap().name == name.as_bytes()
				})
				.unwrap_or_else(|| panic!("missing method call {name}"));
			assert_eq!(
				r.confidence,
				b"unresolved".to_vec(),
				"{name} has an identifier receiver and should stay actionable"
			);
		}
	}

	#[test]
	fn extract_project_receiver_method_with_std_like_name_stays_unresolved() {
		let src = r#"
            struct Repo;
            fn demo(repo: Repo) {
                repo.get();
                repo.insert();
            }
        "#;
		let g = extract("util.rs", src, &make_anchor(), false);
		for name in ["get", "insert"] {
			let r = g
				.refs()
				.find(|r| {
					r.kind == b"method_call"
						&& r.target.as_view().segments().last().unwrap().name == name.as_bytes()
				})
				.unwrap_or_else(|| panic!("missing method call {name}"));
			assert_eq!(r.confidence, b"unresolved".to_vec());
			let mut segs = r.target.as_view().segments();
			assert_ne!(segs.next().unwrap().kind, b"external_pkg");
		}
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
	fn extract_free_call_to_same_file_def_is_resolved() {
		let src = "pub fn run() { foo(); }\npub fn foo() {}";
		let g = extract("util.rs", src, &make_anchor(), true);
		let r = g
			.refs()
			.find(|r| r.kind == b"calls")
			.expect("missing calls ref");
		assert_eq!(
			r.confidence,
			b"resolved",
			"same-file free fn call must be resolved; got {:?}",
			std::str::from_utf8(&r.confidence)
		);
	}

	#[test]
	fn extract_free_call_to_unknown_name_stays_unresolved() {
		let src = "pub fn run() { foo(); }";
		let g = extract("util.rs", src, &make_anchor(), true);
		let r = g
			.refs()
			.find(|r| r.kind == b"calls")
			.expect("missing calls ref");
		assert_eq!(
			r.confidence,
			b"unresolved",
			"call to unknown name stays unresolved; got {:?}",
			std::str::from_utf8(&r.confidence)
		);
	}

	#[test]
	fn extract_self_method_call_to_same_impl_is_resolved() {
		let src = r#"
pub struct W;
impl W {
    fn dispatch(&self) { self.walk(); }
    fn walk(&self) {}
}
"#;
		let g = extract("util.rs", src, &make_anchor(), true);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("missing method_call ref");
		assert_eq!(
			r.confidence,
			b"resolved",
			"self method call to same impl must be resolved; got {:?}",
			std::str::from_utf8(&r.confidence)
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
