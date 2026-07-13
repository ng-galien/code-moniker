use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;

use crate::lang::KindSpec;

pub mod build;
mod kinds;
mod sdk_pipeline;

#[derive(Clone, Debug, Default)]
pub struct Presets {}

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_go::LANGUAGE.into();
	parser.set_language(&language).unwrap_or_else(|err| {
		panic!("failed to load tree-sitter Go grammar: {err}");
	});
	parser.parse(source, None).unwrap_or_else(|| {
		panic!("tree-sitter parse returned None on a non-cancelled call");
	})
}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	sdk_pipeline::extract(uri, source, anchor, deep, presets)
}

pub fn builtin_external_root(root: &str) -> bool {
	sdk_pipeline::STDLIB_PACKAGES.binary_search(&root).is_ok()
}

pub struct Lang;

const DEF_KINDS: &[&str] = &[
	"type",
	"struct",
	"interface",
	"func",
	"method",
	"field",
	"var",
	"const",
];

const DEF_KIND_SPECS: &[KindSpec] = &[
	KindSpec::new("struct", Shape::Type, 20, "struct"),
	KindSpec::new("interface", Shape::Type, 21, "interface"),
	KindSpec::new("type", Shape::Type, 22, "type"),
	KindSpec::new("field", Shape::Value, 30, "field"),
	KindSpec::new("func", Shape::Callable, 40, "func"),
	KindSpec::new("method", Shape::Callable, 41, "method"),
	KindSpec::new("const", Shape::Value, 60, "const"),
	KindSpec::new("var", Shape::Value, 61, "var"),
];

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "go";
	const ALLOWED_KINDS: &'static [&'static str] = DEF_KINDS;
	const KIND_SPECS: &'static [KindSpec] = DEF_KIND_SPECS;
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &["public", "module"];

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

	fn make_anchor() -> Moniker {
		MonikerBuilder::new().project(b"app").build()
	}

	fn extract_default(uri: &str, source: &str, anchor: &Moniker, deep: bool) -> CodeGraph {
		let g = extract(uri, source, anchor, deep, &Presets::default());
		assert_conformance::<super::Lang>(&g, anchor);
		g
	}

	#[test]
	fn parse_empty_returns_source_file() {
		let tree = parse("");
		assert_eq!(tree.root_node().kind(), "source_file");
	}

	#[test]
	fn extract_module_uses_path_segments() {
		let g = extract_default("acme/util/text.go", "package text\n", &make_anchor(), false);
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"package", b"acme")
			.segment(b"package", b"util")
			.segment(b"module", b"text")
			.build();
		assert_eq!(g.root(), &expected);
	}

	#[test]
	fn extract_method_when_type_declared_after_method() {
		let src = "package foo\nfunc (r *Foo) Bar() {}\ntype Foo struct{}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let bar = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"struct", b"Foo")
			.segment(b"method", b"Bar()")
			.build();
		assert!(
			g.contains(&bar),
			"method emitted before its type declaration must still be reparented; defs: {:?}",
			g.def_monikers()
		);
		let bar_def = g.defs().find(|d| d.moniker == bar).expect("method def");
		let defs: Vec<_> = g.defs().collect();
		let parent = bar_def
			.parent
			.and_then(|idx| defs.get(idx).copied())
			.expect("method parent");
		let foo = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"struct", b"Foo")
			.build();
		assert_eq!(
			parent.moniker, foo,
			"method parent metadata must point at receiver type"
		);
	}

	#[test]
	fn extract_simple_call_to_unresolved_callee_uses_name_only() {
		let src = "package foo\nfunc Run() { Helper(1, 2) }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"Helper"
			})
			.expect("calls Helper (name-only, no parens)");
		assert_eq!(r.confidence, b"name_match".to_vec());
	}

	#[test]
	fn extract_composite_literal_unresolved_type_marks_name_match() {
		let src = "package foo\nfunc Run() { _ = Bar{} }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates")
			.expect("instantiates ref");
		assert_eq!(r.confidence, b"name_match".to_vec());
	}

	#[test]
	fn extract_shallow_skips_param_and_local_defs() {
		let src = "package foo\nfunc Run(x int) { y := 1; _ = y }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		assert!(
			g.defs().all(|d| d.kind != b"param" && d.kind != b"local"),
			"shallow extraction must not emit param/local defs"
		);
	}

	#[test]
	fn extract_deep_emits_param_defs_under_function() {
		let src = "package foo\nfunc Run(a int, b string) {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), true);
		let pa = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"func", b"Run(a:int,b:string)")
			.segment(b"param", b"a")
			.build();
		let pb = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"func", b"Run(a:int,b:string)")
			.segment(b"param", b"b")
			.build();
		assert!(g.contains(&pa));
		assert!(g.contains(&pb));
	}

	#[test]
	fn extract_deep_emits_receiver_param_for_method() {
		let src = "package foo\ntype Foo struct{}\nfunc (r *Foo) Bar(x int) {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), true);
		let recv = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"struct", b"Foo")
			.segment(b"method", b"Bar(x:int)")
			.segment(b"param", b"r")
			.build();
		assert!(g.contains(&recv));
	}

	#[test]
	fn extract_deep_skips_blank_param() {
		let src = "package foo\nfunc Run(_ int, b string) {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), true);
		let params: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"param")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert_eq!(params, vec![&b"b"[..]]);
	}

	#[test]
	fn extract_deep_emits_local_def_for_short_var() {
		let src = "package foo\nfunc Run() { x := 1; _ = x }\n";
		let g = extract_default("foo.go", src, &make_anchor(), true);
		let lx = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"func", b"Run()")
			.segment(b"local", b"x")
			.build();
		assert!(g.contains(&lx));
	}

	#[test]
	fn extract_deep_emits_local_defs_for_multi_assign() {
		let src = "package foo\nfunc Run() { x, y := 1, 2; _, _ = x, y }\n";
		let g = extract_default("foo.go", src, &make_anchor(), true);
		let names: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"local")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert!(names.contains(&&b"x"[..]));
		assert!(names.contains(&&b"y"[..]));
	}

	#[test]
	fn extract_deep_emits_local_def_for_var_declaration() {
		let src = "package foo\nfunc Run() { var z int = 5; _ = z }\n";
		let g = extract_default("foo.go", src, &make_anchor(), true);
		let lz = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"func", b"Run()")
			.segment(b"local", b"z")
			.build();
		assert!(g.contains(&lz));
	}

	#[test]
	fn extract_deep_emits_local_defs_for_range_vars() {
		let src =
			"package foo\nfunc Run(m map[string]int) { for k, v := range m { _, _ = k, v } }\n";
		let g = extract_default("foo.go", src, &make_anchor(), true);
		let names: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"local")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert!(names.contains(&&b"k"[..]));
		assert!(names.contains(&&b"v"[..]));
	}

	#[test]
	fn extract_top_level_var_does_not_pollute_locals() {
		let src = "package foo\nvar GlobalCount int\nfunc Run() { GlobalCount = 1 }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let local_names: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"local")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert!(
			local_names.is_empty(),
			"a package-level var must not be emitted as a local. found locals: {:?}",
			local_names
		);
		let vars: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"var")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert_eq!(vars, vec![&b"GlobalCount"[..]]);
	}

	#[test]
	fn extract_deep_skips_blank_in_short_var() {
		let src = "package foo\nfunc Run() { _, y := 1, 2; _ = y }\n";
		let g = extract_default("foo.go", src, &make_anchor(), true);
		let names: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"local")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert_eq!(names, vec![&b"y"[..]]);
	}
}
