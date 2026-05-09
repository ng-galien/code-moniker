use std::cell::RefCell;
use std::collections::HashMap;

use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

pub mod build;
mod canonicalize;
mod kinds;
mod refs;
mod scope;
mod walker;

use canonicalize::compute_module_moniker;
use walker::{ImportEntry, Walker, collect_type_table};

#[derive(Clone, Debug, Default)]
pub struct Presets {}

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_go::LANGUAGE.into();
	parser
		.set_language(&language)
		.expect("failed to load tree-sitter Go grammar");
	parser
		.parse(source, None)
		.expect("tree-sitter parse returned None on a non-cancelled call")
}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let tree = parse(source);
	let module = compute_module_moniker(anchor, uri);
	let mut graph = CodeGraph::new(module.clone(), kinds::MODULE);
	let mut type_table: HashMap<&[u8], Moniker> = HashMap::new();
	collect_type_table(
		tree.root_node(),
		source.as_bytes(),
		&module,
		&mut graph,
		&mut type_table,
	);
	let walker = Walker {
		source_bytes: source.as_bytes(),
		module: module.clone(),
		deep,
		local_scope: RefCell::new(Vec::new()),
		imports: RefCell::new(HashMap::<&[u8], ImportEntry>::new()),
		type_table,
	};
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

pub struct Lang;

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "go";
	const ALLOWED_KINDS: &'static [&'static str] = &[
		"type",
		"struct",
		"interface",
		"func",
		"method",
		"var",
		"const",
	];
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
	fn extract_module_root_is_filename_only() {
		let g = extract_default("foo.go", "package foo\n", &make_anchor(), false);
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.build();
		assert_eq!(g.root(), &expected);
	}

	#[test]
	fn extract_function_with_typed_params_emits_full_signature() {
		let src = "package foo\nfunc Add(a int, b int) int { return a + b }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let f = g.defs().find(|d| d.kind == b"func").expect("function def");
		let last = f.moniker.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"func");
		assert_eq!(last.name, b"Add(int,int)");
		assert_eq!(f.signature, b"int,int".to_vec());
	}

	#[test]
	fn extract_function_grouped_param_names_repeat_type() {
		let src = "package foo\nfunc Add(a, b int) int { return a + b }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let f = g.defs().find(|d| d.kind == b"func").expect("function def");
		assert_eq!(
			f.moniker.as_view().segments().last().unwrap().name,
			b"Add(int,int)",
			"`a, b int` must expand to two int slots"
		);
	}

	#[test]
	fn extract_function_no_params_emits_empty_parens() {
		let src = "package foo\nfunc Boot() {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let f = g.defs().find(|d| d.kind == b"func").expect("function def");
		assert_eq!(
			f.moniker.as_view().segments().last().unwrap().name,
			b"Boot()"
		);
	}

	#[test]
	fn extract_function_capitalized_name_is_public() {
		let src = "package foo\nfunc Hello() {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let f = g.defs().find(|d| d.kind == b"func").expect("function def");
		assert_eq!(f.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_function_lowercase_name_is_module() {
		let src = "package foo\nfunc helper() {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let f = g.defs().find(|d| d.kind == b"func").expect("function def");
		assert_eq!(f.visibility, b"module".to_vec());
	}

	#[test]
	fn extract_variadic_function_emits_ellipsis_slot() {
		let src = "package foo\nfunc Printf(args ...int) {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let f = g.defs().find(|d| d.kind == b"func").expect("function def");
		assert_eq!(
			f.moniker.as_view().segments().last().unwrap().name,
			b"Printf(...)"
		);
	}

	#[test]
	fn extract_struct_emits_struct_def() {
		let src = "package foo\ntype Foo struct { X int }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let foo = g.defs().find(|d| d.kind == b"struct").expect("struct def");
		assert_eq!(
			foo.moniker.as_view().segments().last().unwrap().name,
			b"Foo"
		);
		assert_eq!(foo.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_interface_emits_interface_def() {
		let src = "package foo\ntype Greeter interface { Hello() string }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let i = g
			.defs()
			.find(|d| d.kind == b"interface")
			.expect("interface def");
		assert_eq!(
			i.moniker.as_view().segments().last().unwrap().name,
			b"Greeter"
		);
	}

	#[test]
	fn extract_defined_type_emits_type_alias_def() {
		let src = "package foo\ntype UserID int\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let t = g
			.defs()
			.find(|d| d.kind == b"type")
			.expect("type_alias def");
		assert_eq!(
			t.moniker.as_view().segments().last().unwrap().name,
			b"UserID"
		);
	}

	#[test]
	fn extract_type_alias_with_equals_emits_type_alias_def() {
		let src = "package foo\ntype UserID = int\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let t = g
			.defs()
			.find(|d| d.kind == b"type")
			.expect("type_alias def");
		assert_eq!(
			t.moniker.as_view().segments().last().unwrap().name,
			b"UserID"
		);
	}

	#[test]
	fn extract_grouped_type_decl_emits_each_def() {
		let src = "package foo\ntype (\n\tFoo struct{}\n\tBar interface{}\n)\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		assert!(g.defs().any(|d| d.kind == b"struct"
			&& d.moniker.as_view().segments().last().unwrap().name == b"Foo"));
		assert!(g.defs().any(|d| d.kind == b"interface"
			&& d.moniker.as_view().segments().last().unwrap().name == b"Bar"));
	}

	#[test]
	fn extract_unexported_struct_visibility_is_module() {
		let src = "package foo\ntype internal struct{}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let t = g.defs().find(|d| d.kind == b"struct").expect("struct def");
		assert_eq!(t.visibility, b"module".to_vec());
	}

	#[test]
	fn extract_method_reparents_to_receiver_type() {
		let src = "package foo\ntype Foo struct{}\nfunc (r Foo) Bar(x int) int { return x }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let bar = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"struct", b"Foo")
			.segment(b"method", b"Bar(int)")
			.build();
		assert!(
			g.contains(&bar),
			"expected {bar:?}, defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_method_with_pointer_receiver_strips_star() {
		let src = "package foo\ntype Foo struct{}\nfunc (r *Foo) Bar() {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let bar = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"struct", b"Foo")
			.segment(b"method", b"Bar()")
			.build();
		assert!(g.contains(&bar));
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
	}

	#[test]
	fn extract_method_signature_excludes_receiver() {
		let src =
			"package foo\ntype Foo struct{}\nfunc (r Foo) Sum(a, b int) int { return a + b }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(
			m.moniker.as_view().segments().last().unwrap().name,
			b"Sum(int,int)"
		);
		assert_eq!(m.signature, b"int,int".to_vec());
	}

	#[test]
	fn extract_method_capitalized_visibility_public() {
		let src = "package foo\ntype Foo struct{}\nfunc (f Foo) Public() {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(m.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_method_lowercase_visibility_module() {
		let src = "package foo\ntype Foo struct{}\nfunc (f Foo) hidden() {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(m.visibility, b"module".to_vec());
	}

	#[test]
	fn extract_import_simple_emits_imports_module() {
		let g = extract_default(
			"foo.go",
			"package foo\nimport \"fmt\"\n",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		let target = MonikerBuilder::new()
			.project(b"app")
			.segment(b"external_pkg", b"fmt")
			.build();
		assert_eq!(r.target, target);
		assert_eq!(r.confidence, b"external".to_vec());
		assert!(r.alias.is_empty());
	}

	#[test]
	fn extract_import_path_with_slashes_uses_path_segments() {
		let g = extract_default(
			"foo.go",
			"package foo\nimport \"net/http\"\n",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		let target = MonikerBuilder::new()
			.project(b"app")
			.segment(b"external_pkg", b"net")
			.segment(b"path", b"http")
			.build();
		assert_eq!(r.target, target);
		assert_eq!(r.confidence, b"external".to_vec());
	}

	#[test]
	fn extract_import_third_party_marks_imported() {
		let g = extract_default(
			"foo.go",
			"package foo\nimport \"github.com/gorilla/mux\"\n",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		assert_eq!(r.confidence, b"imported".to_vec());
		let target = MonikerBuilder::new()
			.project(b"app")
			.segment(b"external_pkg", b"github.com")
			.segment(b"path", b"gorilla")
			.segment(b"path", b"mux")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_import_alias_records_alias_attr() {
		let g = extract_default(
			"foo.go",
			"package foo\nimport f \"fmt\"\n",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		assert_eq!(r.alias, b"f".to_vec());
	}

	#[test]
	fn extract_import_dot_emits_dot_alias() {
		let g = extract_default(
			"foo.go",
			"package foo\nimport . \"fmt\"\n",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		assert_eq!(r.alias, b".".to_vec());
	}

	#[test]
	fn extract_import_blank_emits_underscore_alias() {
		let g = extract_default(
			"foo.go",
			"package foo\nimport _ \"fmt\"\n",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		assert_eq!(r.alias, b"_".to_vec());
	}

	#[test]
	fn extract_grouped_imports_emit_one_ref_per_spec() {
		let src = "package foo\nimport (\n\t\"fmt\"\n\t\"os\"\n\t\"github.com/x/y\"\n)\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let count = g.refs().filter(|r| r.kind == b"imports_module").count();
		assert_eq!(count, 3);
	}

	#[test]
	fn extract_simple_call_emits_calls_with_arity() {
		let src = "package foo\nfunc Run() { Helper(1, 2) }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"Helper(2)"
			})
			.expect("calls Helper(2)");
		assert_eq!(r.confidence, b"name_match".to_vec());
	}

	#[test]
	fn extract_imported_call_uses_full_path_in_target() {
		let src = "package foo\nimport \"net/http\"\nfunc Run() { http.Get(\"u\") }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"Get(1)"
			})
			.expect("calls http.Get");
		assert_eq!(r.confidence, b"external".to_vec());
		let segs: Vec<&[u8]> = r.target.as_view().segments().map(|s| s.kind).collect();
		assert_eq!(segs, vec![&b"external_pkg"[..], &b"path"[..], &b"func"[..]]);
	}

	#[test]
	fn extract_imported_simple_path_call_target() {
		let src = "package foo\nimport \"fmt\"\nfunc Run() { fmt.Println(1) }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"Println(1)"
			})
			.expect("calls fmt.Println");
		let names: Vec<&[u8]> = r.target.as_view().segments().map(|s| s.name).collect();
		assert_eq!(names, vec![&b"fmt"[..], &b"Println(1)"[..]]);
	}

	#[test]
	fn extract_third_party_call_marks_imported() {
		let src = "package foo\nimport \"github.com/x/mux\"\nfunc Run() { mux.New() }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls" && r.target.as_view().segments().last().unwrap().name == b"New()"
			})
			.expect("calls mux.New");
		assert_eq!(r.confidence, b"imported".to_vec());
	}

	#[test]
	fn extract_method_call_carries_receiver_hint_identifier() {
		let src = "package foo\nfunc Run(obj T) { obj.Bar() }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("method_call ref");
		assert_eq!(r.receiver_hint, b"obj".to_vec());
		assert_eq!(r.target.as_view().segments().last().unwrap().name, b"Bar()");
	}

	#[test]
	fn extract_chained_method_call_receiver_hint_is_call() {
		let src = "package foo\nfunc Run() { foo().bar() }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"bar()"
			})
			.expect("method_call bar");
		assert_eq!(r.receiver_hint, b"call".to_vec());
	}

	#[test]
	fn extract_call_visits_arguments_for_nested_calls() {
		let src = "package foo\nfunc Run() { Outer(Inner()) }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let names: Vec<&[u8]> = g
			.refs()
			.filter(|r| r.kind == b"calls")
			.map(|r| r.target.as_view().segments().last().unwrap().name)
			.collect();
		assert!(names.contains(&&b"Outer(1)"[..]));
		assert!(names.contains(&&b"Inner()"[..]));
	}

	#[test]
	fn extract_composite_literal_emits_instantiates() {
		let src = "package foo\ntype Foo struct{ X int }\nfunc Run() { _ = Foo{X: 1} }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates")
			.expect("instantiates ref");
		assert_eq!(r.target.as_view().segments().last().unwrap().name, b"Foo");
		assert_eq!(r.confidence, b"resolved".to_vec());
	}

	#[test]
	fn extract_qualified_composite_literal_uses_imported_path() {
		let src = "package foo\nimport \"net/http\"\nfunc Run() { _ = http.Request{} }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates")
			.expect("instantiates ref");
		let names: Vec<&[u8]> = r.target.as_view().segments().map(|s| s.name).collect();
		assert_eq!(names, vec![&b"net"[..], &b"http"[..], &b"Request"[..]]);
		assert_eq!(r.confidence, b"external".to_vec());
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
	fn extract_param_type_emits_uses_type() {
		let src = "package foo\ntype Bar struct{}\nfunc Run(x Bar) {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"uses_type"
					&& r.target.as_view().segments().last().unwrap().name == b"Bar"
			})
			.expect("uses_type Bar");
		assert_eq!(r.confidence, b"resolved".to_vec());
	}

	#[test]
	fn extract_return_type_emits_uses_type() {
		let src = "package foo\ntype Bar struct{}\nfunc Run() Bar { return Bar{} }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		assert!(g.refs().any(|r| r.kind == b"uses_type"
			&& r.target.as_view().segments().last().unwrap().name == b"Bar"));
	}

	#[test]
	fn extract_multi_return_emits_uses_type_for_each() {
		let src =
			"package foo\ntype A struct{}\ntype B struct{}\nfunc Run() (a A, b B) { return }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let names: Vec<&[u8]> = g
			.refs()
			.filter(|r| r.kind == b"uses_type")
			.map(|r| r.target.as_view().segments().last().unwrap().name)
			.collect();
		assert!(names.contains(&&b"A"[..]));
		assert!(names.contains(&&b"B"[..]));
	}

	#[test]
	fn extract_qualified_param_type_uses_imported_path() {
		let src = "package foo\nimport \"net/http\"\nfunc Run(r http.Request) {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"uses_type"
					&& r.target.as_view().segments().last().unwrap().name == b"Request"
			})
			.expect("uses_type Request");
		assert_eq!(r.confidence, b"external".to_vec());
		let names: Vec<&[u8]> = r.target.as_view().segments().map(|s| s.name).collect();
		assert_eq!(names, vec![&b"net"[..], &b"http"[..], &b"Request"[..]]);
	}

	#[test]
	fn extract_pointer_param_type_descends() {
		let src = "package foo\ntype Bar struct{}\nfunc Run(x *Bar) {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		assert!(g.refs().any(|r| r.kind == b"uses_type"
			&& r.target.as_view().segments().last().unwrap().name == b"Bar"));
	}

	#[test]
	fn extract_slice_param_type_descends() {
		let src = "package foo\ntype Bar struct{}\nfunc Run(xs []Bar) {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		assert!(g.refs().any(|r| r.kind == b"uses_type"
			&& r.target.as_view().segments().last().unwrap().name == b"Bar"));
	}

	#[test]
	fn extract_map_param_type_descends_into_key_and_value() {
		let src = "package foo\ntype K struct{}\ntype V struct{}\nfunc Run(m map[K]V) {}\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let names: Vec<&[u8]> = g
			.refs()
			.filter(|r| r.kind == b"uses_type")
			.map(|r| r.target.as_view().segments().last().unwrap().name)
			.collect();
		assert!(names.contains(&&b"K"[..]));
		assert!(names.contains(&&b"V"[..]));
	}

	#[test]
	fn extract_struct_field_type_emits_uses_type() {
		let src = "package foo\ntype Bar struct{}\ntype Foo struct { x Bar }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"uses_type"
					&& r.target.as_view().segments().last().unwrap().name == b"Bar"
			})
			.expect("uses_type Bar");
		let foo = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"struct", b"Foo")
			.build();
		assert_eq!(g.defs().nth(r.source).unwrap().moniker, foo);
	}

	#[test]
	fn extract_struct_embedding_emits_extends() {
		let src = "package foo\ntype Base struct{}\ntype Derived struct { Base; X int }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"extends")
			.expect("extends ref");
		assert_eq!(r.target.as_view().segments().last().unwrap().name, b"Base");
		let derived = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"struct", b"Derived")
			.build();
		assert_eq!(g.defs().nth(r.source).unwrap().moniker, derived);
	}

	#[test]
	fn extract_pointer_embedding_strips_star_for_extends() {
		let src = "package foo\ntype Base struct{}\ntype Derived struct { *Base }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"extends")
			.expect("extends ref");
		assert_eq!(r.target.as_view().segments().last().unwrap().name, b"Base");
	}

	#[test]
	fn extract_qualified_embedding_uses_imported_path() {
		let src = "package foo\nimport \"net/http\"\ntype Wrapper struct { http.Request }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"extends")
			.expect("extends ref");
		assert_eq!(r.confidence, b"external".to_vec());
		let names: Vec<&[u8]> = r.target.as_view().segments().map(|s| s.name).collect();
		assert_eq!(names, vec![&b"net"[..], &b"http"[..], &b"Request"[..]]);
	}

	#[test]
	fn extract_interface_embedding_emits_extends() {
		let src = "package foo\ntype Reader interface { Read() }\ntype ReadCloser interface { Reader; Close() }\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"extends")
			.expect("extends ref");
		assert_eq!(
			r.target.as_view().segments().last().unwrap().name,
			b"Reader"
		);
	}

	#[test]
	fn extract_type_alias_emits_uses_type_on_underlying() {
		let src = "package foo\ntype Bar struct{}\ntype Aliased = Bar\n";
		let g = extract_default("foo.go", src, &make_anchor(), false);
		assert!(g.refs().any(|r| r.kind == b"uses_type"
			&& r.target.as_view().segments().last().unwrap().name == b"Bar"));
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
			.segment(b"func", b"Run(int,string)")
			.segment(b"param", b"a")
			.build();
		let pb = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"go")
			.segment(b"module", b"foo")
			.segment(b"func", b"Run(int,string)")
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
			.segment(b"method", b"Bar(int)")
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
