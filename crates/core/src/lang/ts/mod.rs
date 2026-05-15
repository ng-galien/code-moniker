use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;

use crate::lang::KindSpec;
use crate::lang::canonical_walker::CanonicalWalker;

pub mod build;
mod canonicalize;
mod kinds;
mod strategy;

use canonicalize::compute_module_moniker;
use strategy::{CallableEntry, Strategy, collect_callable_table, collect_export_ranges};

pub fn parse(source: &str) -> Tree {
	parse_with_uri(source, "")
}

pub fn parse_with_uri(source: &str, uri: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = if uri_uses_jsx(uri) {
		tree_sitter_typescript::LANGUAGE_TSX.into()
	} else {
		tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
	};
	parser
		.set_language(&language)
		.expect("failed to load tree-sitter TypeScript grammar");
	parser
		.parse(source, None)
		.expect("tree-sitter parse returned None on a non-cancelled call")
}

fn uri_uses_jsx(uri: &str) -> bool {
	uri.ends_with(".tsx") || uri.ends_with(".jsx")
}

#[derive(Clone, Debug, Default)]
pub struct Presets {
	pub di_register_callees: Vec<String>,
	pub path_aliases: Vec<PathAlias>,
}

#[derive(Clone, Debug)]
pub struct PathAlias {
	pub pattern: String,
	pub substitution: String,
}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let (def_cap, ref_cap) = CodeGraph::capacity_for_source(source.len());
	let mut graph = CodeGraph::with_capacity(module.clone(), kinds::MODULE, def_cap, ref_cap);
	let tree = parse_with_uri(source, uri);
	let export_ranges = collect_export_ranges(tree.root_node());
	let mut callable_table: std::collections::HashMap<(Moniker, Vec<u8>), CallableEntry> =
		std::collections::HashMap::new();
	collect_callable_table(
		tree.root_node(),
		source.as_bytes(),
		&module,
		&mut callable_table,
	);
	let strat = Strategy {
		module: module.clone(),
		anchor: anchor.clone(),
		source_bytes: source.as_bytes(),
		deep,
		presets,
		export_ranges,
		local_scope: std::cell::RefCell::new(Vec::new()),
		imports: std::cell::RefCell::new(std::collections::HashMap::new()),
		import_targets: std::cell::RefCell::new(std::collections::HashMap::new()),
		callable_table,
		nested_funcs: std::cell::RefCell::new(Vec::new()),
	};
	let walker = CanonicalWalker::new(&strat, source.as_bytes());
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

pub struct Lang;

const DEF_KINDS: &[&str] = &[
	"class",
	"interface",
	"type",
	"function",
	"method",
	"const",
	"enum",
	"constructor",
	"field",
	"enum_constant",
	"namespace",
];

const DEF_KIND_SPECS: &[KindSpec] = &[
	KindSpec::new("namespace", Shape::Namespace, 10, "namespace"),
	KindSpec::new("class", Shape::Type, 20, "class"),
	KindSpec::new("interface", Shape::Type, 21, "interface"),
	KindSpec::new("enum", Shape::Type, 22, "enum"),
	KindSpec::new("type", Shape::Type, 23, "type"),
	KindSpec::new("constructor", Shape::Callable, 40, "constructor"),
	KindSpec::new("method", Shape::Callable, 41, "method"),
	KindSpec::new("function", Shape::Callable, 42, "function"),
	KindSpec::new("field", Shape::Value, 60, "field"),
	KindSpec::new("enum_constant", Shape::Value, 61, "enum_constant"),
	KindSpec::new("const", Shape::Value, 62, "const"),
];

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "ts";
	const ALLOWED_KINDS: &'static [&'static str] = DEF_KINDS;
	const KIND_SPECS: &'static [KindSpec] = DEF_KIND_SPECS;
	const ALLOWED_VISIBILITIES: &'static [&'static str] =
		&["public", "private", "protected", "module"];

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
	use crate::core::moniker::MonikerBuilder;
	use crate::lang::assert_conformance;

	fn extract(uri: &str, source: &str, anchor: &Moniker, deep: bool) -> CodeGraph {
		let g = super::extract(uri, source, anchor, deep, &Presets::default());
		assert_conformance::<super::Lang>(&g, anchor);
		g
	}

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
	fn extract_strips_each_known_extension() {
		let anchor = make_anchor();
		for uri in [
			"foo.ts", "foo.tsx", "foo.js", "foo.jsx", "foo.mjs", "foo.cjs",
		] {
			let g = extract(uri, "", &anchor, false);
			let last = g.root().as_view().segments().last().unwrap();
			assert_eq!(last.name, b"foo", "extension not stripped on {uri}");
		}
	}

	#[test]
	fn extract_dot_only_specifier_resolves_relative_not_external() {
		let g = extract(
			"src/__tests__/foo.test.ts",
			"import { z } from \"..\";",
			&make_anchor(),
			false,
		);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"dir", b"src")
			.segment(b"path", b"z")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_dotdot_import_walks_up_then_down() {
		let g = extract(
			"src/lib/foo.ts",
			"import { X } from '../other';",
			&make_anchor(),
			false,
		);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"dir", b"src")
			.segment(b"module", b"other")
			.segment(b"path", b"X")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_call_to_nested_function_is_resolved() {
		let src = r#"
function outer() {
    function inner() {}
    inner();
}
"#;
		let g = extract("util.ts", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target
						.as_view()
						.segments()
						.last()
						.unwrap()
						.name
						.starts_with(b"inner")
			})
			.expect("calls ref for inner");
		assert_eq!(
			r.confidence,
			b"resolved",
			"call to nested fn must be resolved; got {:?}",
			std::str::from_utf8(&r.confidence)
		);
		let segs: Vec<_> = r.target.as_view().segments().collect();
		assert!(
			segs.iter()
				.any(|s| s.kind == b"function" && s.name.starts_with(b"outer")),
			"target must be scoped under outer; got {:?}",
			segs.iter()
				.map(|s| (
					std::str::from_utf8(s.kind).unwrap_or("?"),
					std::str::from_utf8(s.name).unwrap_or("?")
				))
				.collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_call_hoists_nested_fn_used_before_decl() {
		let src = r#"
function outer() {
    inner();
    function inner() {}
}
"#;
		let g = extract("util.ts", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target
						.as_view()
						.segments()
						.last()
						.unwrap()
						.name
						.starts_with(b"inner")
			})
			.expect("calls ref for inner");
		assert_eq!(
			r.confidence,
			b"resolved",
			"hoisted nested fn call must be resolved; got {:?}",
			std::str::from_utf8(&r.confidence)
		);
	}

	#[test]
	fn extract_reads_param_marks_confidence_local() {
		let g = extract(
			"util.ts",
			"function f(x) { return x; }",
			&make_anchor(),
			true,
		);
		let r = g.refs().find(|r| r.kind == b"reads").expect("reads ref");
		assert_eq!(r.confidence, b"local".to_vec(), "ref to a param is local");
	}

	#[test]
	fn extract_calls_local_function_marks_confidence_local() {
		let g = extract(
			"util.ts",
			"function f() { const helper = () => 1; helper(); }",
			&make_anchor(),
			true,
		);
		let r = g.refs().find(|r| r.kind == b"calls").expect("calls ref");
		assert_eq!(
			r.confidence,
			b"local".to_vec(),
			"call into a locally-bound name is local"
		);
	}

	#[test]
	fn extract_local_def_has_no_visibility() {
		let g = extract(
			"util.ts",
			"function f() { let x = 1; }",
			&make_anchor(),
			true,
		);
		let local = g.defs().find(|d| d.kind == b"local").expect("local def");
		assert!(
			local.visibility.is_empty(),
			"locals must not carry a synthetic visibility, got {:?}",
			String::from_utf8_lossy(&local.visibility)
		);
	}

	#[test]
	fn extract_param_def_has_no_visibility() {
		let g = extract("util.ts", "function f(x) {}", &make_anchor(), true);
		let p = g.defs().find(|d| d.kind == b"param").expect("param def");
		assert!(p.visibility.is_empty());
	}

	#[test]
	fn extract_di_register_fires_only_when_callee_in_preset() {
		let presets = Presets {
			di_register_callees: vec!["register".into(), "bind".into()],
			..Presets::default()
		};
		let g = super::extract(
			"util.ts",
			"register(UserService);",
			&make_anchor(),
			false,
			&presets,
		);
		assert!(g.refs().any(|r| r.kind == b"di_register"));
	}

	#[test]
	fn extract_di_register_silent_without_preset() {
		let g = extract("util.ts", "register(UserService);", &make_anchor(), false);
		assert!(
			g.refs().all(|r| r.kind != b"di_register"),
			"di_register must stay silent without a preset",
		);
	}

	#[test]
	fn extract_di_register_skips_non_matching_callee() {
		let presets = Presets {
			di_register_callees: vec!["register".into()],
			..Presets::default()
		};
		let g = super::extract("util.ts", "expect(value);", &make_anchor(), false, &presets);
		assert!(g.refs().all(|r| r.kind != b"di_register"));
	}

	#[test]
	fn extract_di_register_register_with_name_and_factory() {
		let presets = Presets {
			di_register_callees: vec!["register".into()],
			..Presets::default()
		};
		let g = super::extract(
			"util.ts",
			"register('repoStore', makeRepoStore);",
			&make_anchor(),
			false,
			&presets,
		);
		assert!(
			g.refs().any(|r| r.kind == b"di_register"),
			"register('name', factory) must emit di_register on the factory identifier",
		);
	}

	#[test]
	fn extract_di_register_member_callee_register() {
		let presets = Presets {
			di_register_callees: vec!["register".into()],
			..Presets::default()
		};
		let g = super::extract(
			"util.ts",
			"container.register('repoStore', makeRepoStore);",
			&make_anchor(),
			false,
			&presets,
		);
		assert!(
			g.refs().any(|r| r.kind == b"di_register"),
			"container.register(...) must emit di_register when 'register' is in the preset",
		);
	}

	#[test]
	fn extract_di_register_recurses_into_factory_call_argument() {
		let presets = Presets {
			di_register_callees: vec!["register".into()],
			..Presets::default()
		};
		let g = super::extract(
			"util.ts",
			"register('repoStore', asFunction(makeRepoStore));",
			&make_anchor(),
			false,
			&presets,
		);
		assert!(
			g.refs().any(|r| r.kind == b"di_register"),
			"register('name', asFunction(make)) must recurse to find 'make'",
		);
	}

	#[test]
	fn extract_di_register_recurses_through_chained_call_postfix() {
		let presets = Presets {
			di_register_callees: vec!["asFunction".into()],
			..Presets::default()
		};
		let g = super::extract(
			"util.ts",
			"asFunction(makeRepoStore).singleton();",
			&make_anchor(),
			false,
			&presets,
		);
		assert!(
			g.refs().any(|r| r.kind == b"di_register"),
			"asFunction(make).singleton() chain must still register the inner 'make'",
		);
	}

	#[test]
	fn extract_di_register_full_awilix_pattern() {
		let presets = Presets {
			di_register_callees: vec!["register".into()],
			..Presets::default()
		};
		let g = super::extract(
			"util.ts",
			"container.register('readResource', asFunction(makeReadResource).singleton());",
			&make_anchor(),
			false,
			&presets,
		);
		assert!(
			g.refs().any(|r| r.kind == b"di_register"),
			"container.register('name', asFunction(make).singleton()) must emit di_register",
		);
	}
	#[test]
	fn extract_shallow_skips_param_and_local() {
		let g = extract(
			"util.ts",
			"function f(a: number) { let x = 1; }",
			&make_anchor(),
			false,
		);
		assert!(
			g.defs().all(|d| d.kind != b"param" && d.kind != b"local"),
			"shallow extraction must not produce param/local defs"
		);
	}

	#[test]
	fn extract_deep_emits_params_and_locals() {
		let g = extract(
			"util.ts",
			"function f(a: number, b: number) { let sum = a + b; }",
			&make_anchor(),
			true,
		);
		let pa = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"f(a:number,b:number)")
			.segment(b"param", b"a")
			.build();
		let pb = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"f(a:number,b:number)")
			.segment(b"param", b"b")
			.build();
		let sum = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"f(a:number,b:number)")
			.segment(b"local", b"sum")
			.build();
		assert!(
			g.contains(&pa),
			"missing param a; defs: {:?}",
			g.def_monikers()
		);
		assert!(g.contains(&pb));
		assert!(g.contains(&sum));
	}

	#[test]
	fn extract_deep_anonymous_callback_uses_position_name() {
		let g = extract(
			"util.ts",
			"function f() { [1].map(x => x); }",
			&make_anchor(),
			true,
		);
		let monikers = g.def_monikers();
		let cb = monikers
			.iter()
			.find(|m| {
				let last = m.as_view().segments().last().unwrap();
				last.kind == b"function" && last.name.starts_with(b"__cb_")
			})
			.expect("anonymous callback def with __cb_ prefix")
			.clone();
		let view = cb.as_view();
		let last = view.segments().last().unwrap();
		assert_eq!(last.kind, b"function");
		assert!(g.defs().any(|d| {
			let dv = d.moniker.as_view();
			dv.segment_count() == view.segment_count() + 1
				&& dv.segments().last().unwrap().kind == b"param"
		}));
	}

	#[test]
	fn extract_alias_import_routes_to_project_rooted_module() {
		let presets = Presets {
			path_aliases: vec![PathAlias {
				pattern: "@/*".into(),
				substitution: "./src/*".into(),
			}],
			..Presets::default()
		};
		let g = super::extract(
			"src/router.tsx",
			"import { AppShell } from '@/components/layout/app-shell';",
			&make_anchor(),
			false,
			&presets,
		);
		let r = g.refs().next().expect("one ref");
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"dir", b"src")
			.segment(b"dir", b"components")
			.segment(b"dir", b"layout")
			.segment(b"module", b"app-shell")
			.segment(b"path", b"AppShell")
			.build();
		assert_eq!(
			r.target, target,
			"alias-resolved import must point at the project-rooted module, not external_pkg",
		);
	}

	#[test]
	fn extract_alias_import_keeps_external_when_no_alias_matches() {
		let presets = Presets {
			path_aliases: vec![PathAlias {
				pattern: "@/*".into(),
				substitution: "./src/*".into(),
			}],
			..Presets::default()
		};
		let g = super::extract(
			"util.ts",
			"import { join } from '@scope/pkg/sub';",
			&make_anchor(),
			false,
			&presets,
		);
		let r = g.refs().next().unwrap();
		let head = r.target.as_view().segments().next().unwrap();
		assert_eq!(head.kind, b"external_pkg");
	}

	#[test]
	fn extract_jsx_expression_identifier_still_emits_read() {
		let g = extract(
			"app.tsx",
			"function App(label: string) { return <div>{label}</div>; }",
			&make_anchor(),
			true,
		);
		assert!(
			g.refs().any(|r| r.kind == b"reads"
				&& r.target.as_view().segments().last().unwrap().name == b"label"),
			"identifier inside jsx_expression must still surface as a read",
		);
	}

	#[test]
	fn extract_closure_read_targets_outer_param_def() {
		let src = "function outer({ x }: { x: string }) { return function inner() { return x; }; }";
		let g = extract("util.ts", src, &make_anchor(), true);
		let read = g
			.refs()
			.find(|r| {
				r.kind == b"reads" && r.target.as_view().segments().last().unwrap().name == b"x"
			})
			.expect("reads ref for x");
		let segs: Vec<_> = read.target.as_view().segments().collect();
		assert!(
			segs.iter().any(|s| s.kind == b"param" && s.name == b"x"),
			"target must terminate with param:x of the defining frame, got: {segs:?}"
		);
		assert!(
			!segs
				.iter()
				.any(|s| s.kind == b"function" && s.name == b"inner()"),
			"target must NOT carry the inner frame segment, got: {segs:?}"
		);
	}

	#[test]
	fn extract_closure_uses_type_targets_outer_type_alias_def() {
		let src = "function outer() { type Local = string; function inner(x: Local): Local { return x; } return inner; }";
		let g = extract("util.ts", src, &make_anchor(), true);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"uses_type"
					&& r.target.as_view().segments().last().unwrap().name == b"Local"
			})
			.expect("uses_type ref for Local");
		let segs: Vec<_> = r.target.as_view().segments().collect();
		assert!(
			segs.iter().any(|s| s.kind == b"type" && s.name == b"Local"),
			"target must terminate with type:Local of the defining frame, got: {segs:?}"
		);
		assert!(
			segs.iter()
				.any(|s| s.kind == b"function" && s.name.starts_with(b"outer")),
			"target must be parented under outer (the defining frame), got: {segs:?}"
		);
	}

	#[test]
	fn extract_closure_uses_type_targets_outer_interface_def() {
		let src = "function outer() { interface Local { v: string; } function inner(x: Local): Local { return x; } return inner; }";
		let g = extract("util.ts", src, &make_anchor(), true);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"uses_type"
					&& r.target.as_view().segments().last().unwrap().name == b"Local"
			})
			.expect("uses_type ref for Local");
		let segs: Vec<_> = r.target.as_view().segments().collect();
		assert!(
			segs.iter()
				.any(|s| s.kind == b"interface" && s.name == b"Local"),
			"target must terminate with interface:Local of the defining frame, got: {segs:?}"
		);
		assert!(
			segs.iter()
				.any(|s| s.kind == b"function" && s.name.starts_with(b"outer")),
			"target must be parented under outer, got: {segs:?}"
		);
	}

	#[test]
	fn extract_closure_instantiates_targets_outer_class_def() {
		let src = "function outer() { class Local { ok = true; } function inner() { return new Local(); } return inner; }";
		let g = extract("util.ts", src, &make_anchor(), true);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"instantiates"
					&& r.target.as_view().segments().last().unwrap().name == b"Local"
			})
			.expect("instantiates ref for Local");
		let segs: Vec<_> = r.target.as_view().segments().collect();
		assert!(
			segs.iter()
				.any(|s| s.kind == b"class" && s.name == b"Local"),
			"target must terminate with class:Local of the defining frame, got: {segs:?}"
		);
		assert!(
			segs.iter()
				.any(|s| s.kind == b"function" && s.name.starts_with(b"outer")),
			"target must be parented under outer, got: {segs:?}"
		);
	}

	#[test]
	fn extract_closure_uses_type_targets_outer_enum_def() {
		let src = "function outer() { enum Mode { A, B } function inner(m: Mode): Mode { return m; } return inner; }";
		let g = extract("util.ts", src, &make_anchor(), true);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"uses_type"
					&& r.target.as_view().segments().last().unwrap().name == b"Mode"
			})
			.expect("uses_type ref for Mode");
		let segs: Vec<_> = r.target.as_view().segments().collect();
		assert!(
			segs.iter().any(|s| s.kind == b"enum" && s.name == b"Mode"),
			"target must terminate with enum:Mode of the defining frame, got: {segs:?}"
		);
		assert!(
			segs.iter()
				.any(|s| s.kind == b"function" && s.name.starts_with(b"outer")),
			"target must be parented under outer, got: {segs:?}"
		);
	}

	#[test]
	fn extract_closure_call_targets_outer_local_def() {
		let src = "function outer() { const helper = () => 1; return function inner() { return helper(); }; }";
		let g = extract("util.ts", src, &make_anchor(), true);
		let call = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"helper"
			})
			.expect("calls ref for helper");
		let segs: Vec<_> = call.target.as_view().segments().collect();
		assert!(
			segs.iter()
				.any(|s| s.kind == b"local" && s.name == b"helper"),
			"target must terminate with local:helper of the defining frame, got: {segs:?}"
		);
		assert!(
			!segs
				.iter()
				.any(|s| s.kind == b"function" && s.name == b"inner()"),
			"target must NOT carry the inner frame segment, got: {segs:?}"
		);
	}
}
