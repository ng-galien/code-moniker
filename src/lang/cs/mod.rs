use std::cell::RefCell;
use std::collections::HashMap;

use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use crate::lang::canonical_walker::CanonicalWalker;

pub mod build;
mod canonicalize;
mod kinds;
mod strategy;

use canonicalize::compute_module_moniker;
use strategy::{Strategy, collect_callable_table, collect_type_table};

#[derive(Clone, Debug, Default)]
pub struct Presets {}

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_c_sharp::LANGUAGE.into();
	parser
		.set_language(&language)
		.expect("failed to load tree-sitter C# grammar");
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
		&mut type_table,
	);
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
		imports: RefCell::new(HashMap::new()),
		local_scope: RefCell::new(Vec::new()),
		type_table,
		callable_table,
	};
	let walker = CanonicalWalker::new(&strat, source.as_bytes());
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

pub struct Lang;

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "cs";
	const ALLOWED_KINDS: &'static [&'static str] = &[
		"class",
		"interface",
		"struct",
		"record",
		"enum",
		"delegate",
		"method",
		"constructor",
		"field",
		"property",
		"event",
	];
	const ALLOWED_VISIBILITIES: &'static [&'static str] =
		&["public", "protected", "package", "private"];

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
	fn parse_empty_returns_compilation_unit() {
		let tree = parse("");
		assert_eq!(tree.root_node().kind(), "compilation_unit");
	}

	#[test]
	fn extract_emits_comment_def_per_comment_node() {
		let src = "// a\n/// b\nclass Foo {}\n";
		let g = extract_default("Foo.cs", src, &make_anchor(), false);
		let n = g.defs().filter(|d| d.kind == b"comment").count();
		assert_eq!(n, 2);
	}

	#[test]
	fn extract_module_uses_path_segments() {
		let g = extract_default(
			"Acme/Util/Text.cs",
			"namespace Acme.Util;\n",
			&make_anchor(),
			false,
		);
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"package", b"Acme")
			.segment(b"package", b"Util")
			.segment(b"module", b"Text")
			.build();
		assert_eq!(g.root(), &expected);
	}

	#[test]
	fn extract_class_emits_class_def() {
		let src = "namespace Foo;\npublic class Bar {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let bar = g.defs().find(|d| d.kind == b"class").expect("class def");
		assert_eq!(
			bar.moniker.as_view().segments().last().unwrap().name,
			b"Bar"
		);
		assert_eq!(bar.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_struct_emits_struct_def() {
		let src = "namespace Foo;\npublic struct Bar {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		assert!(g.defs().any(|d| d.kind == b"struct"
			&& d.moniker.as_view().segments().last().unwrap().name == b"Bar"));
	}

	#[test]
	fn extract_interface_emits_interface_def() {
		let src = "namespace Foo;\npublic interface IBar {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let i = g
			.defs()
			.find(|d| d.kind == b"interface")
			.expect("interface def");
		assert_eq!(i.moniker.as_view().segments().last().unwrap().name, b"IBar");
	}

	#[test]
	fn extract_enum_emits_enum_def() {
		let src = "namespace Foo;\npublic enum Color { Red, Green }\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let e = g.defs().find(|d| d.kind == b"enum").expect("enum def");
		assert_eq!(
			e.moniker.as_view().segments().last().unwrap().name,
			b"Color"
		);
	}

	#[test]
	fn extract_top_level_type_default_visibility_is_internal() {
		let src = "namespace Foo;\nclass Bar {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let bar = g.defs().find(|d| d.kind == b"class").expect("class def");
		assert_eq!(
			bar.visibility,
			b"package".to_vec(),
			"top-level C# class without modifier defaults to internal (= VIS_PACKAGE)"
		);
	}

	#[test]
	fn extract_block_namespace_descends_into_body() {
		let src = "namespace Foo {\n    public class Bar {}\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		assert!(g.defs().any(|d| d.kind == b"class"));
	}

	#[test]
	fn extract_method_reparented_under_class() {
		let src = "namespace Foo;\npublic class Bar {\n    public int Add(int a, int b) { return a + b; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"module", b"F")
			.segment(b"class", b"Bar")
			.segment(b"method", b"Add(a:int,b:int)")
			.build();
		assert!(
			g.contains(&m),
			"expected {m:?}, defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_method_signature_excludes_return_type() {
		let src = "namespace Foo;\npublic class Bar {\n    public string Greet(string n) { return n; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(m.signature, b"n:string".to_vec());
	}

	#[test]
	fn extract_method_default_visibility_is_private() {
		let src = "namespace Foo;\npublic class Bar {\n    int Hidden() { return 0; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(m.visibility, b"private".to_vec());
	}

	#[test]
	fn extract_constructor_emits_constructor_def() {
		let src = "namespace Foo;\npublic class Bar {\n    public Bar(int x) {}\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let ctor = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"module", b"F")
			.segment(b"class", b"Bar")
			.segment(b"constructor", b"Bar(x:int)")
			.build();
		assert!(
			g.contains(&ctor),
			"constructor expected at {ctor:?}; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_method_no_params_emits_empty_parens() {
		let src = "namespace Foo;\npublic class Bar {\n    public void Boot() {}\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(
			m.moniker.as_view().segments().last().unwrap().name,
			b"Boot()"
		);
	}

	#[test]
	fn extract_method_params_modifier_emits_ellipsis() {
		let src =
			"namespace Foo;\npublic class Bar {\n    public void Log(params object[] args) {}\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(
			m.moniker.as_view().segments().last().unwrap().name,
			b"Log(...)"
		);
	}

	#[test]
	fn extract_record_emits_record_plus_primary_constructor() {
		let src = "namespace Foo;\npublic record Person(int Age, string Name);\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let record = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"module", b"F")
			.segment(b"record", b"Person")
			.build();
		let ctor = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"module", b"F")
			.segment(b"record", b"Person")
			.segment(b"constructor", b"Person(Age:int,Name:string)")
			.build();
		assert!(g.contains(&record));
		assert!(
			g.contains(&ctor),
			"record primary constructor expected at {ctor:?}; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_nested_class_attached_to_outer_class() {
		let src = "namespace Foo;\npublic class Outer {\n    public class Inner {}\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let inner = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"module", b"F")
			.segment(b"class", b"Outer")
			.segment(b"class", b"Inner")
			.build();
		assert!(g.contains(&inner));
	}

	#[test]
	fn extract_field_emits_field_def() {
		let src = "namespace Foo;\npublic class Bar {\n    private int _count;\n    public string Name = \"x\";\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let names: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"field")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert!(names.contains(&&b"_count"[..]));
		assert!(names.contains(&&b"Name"[..]));
		let count = g
			.defs()
			.find(|d| d.moniker.as_view().segments().last().unwrap().name == b"_count")
			.unwrap();
		assert_eq!(count.visibility, b"private".to_vec());
		let name_def = g
			.defs()
			.find(|d| d.moniker.as_view().segments().last().unwrap().name == b"Name")
			.unwrap();
		assert_eq!(name_def.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_field_with_user_type_emits_uses_type() {
		let src = "namespace Foo;\npublic class Other {}\npublic class Bar {\n    private Other _ref;\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"uses_type"
					&& r.target.as_view().segments().last().unwrap().name == b"Other"
			})
			.expect("uses_type Other");
		assert!(matches!(
			r.confidence.as_slice(),
			b"name_match" | b"resolved"
		));
	}

	#[test]
	fn extract_property_emits_property_def() {
		let src = "namespace Foo;\npublic class Bar {\n    public string Name { get; set; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let p = g
			.defs()
			.find(|d| {
				d.kind == b"property"
					&& d.moniker.as_view().segments().last().unwrap().name == b"Name"
			})
			.expect("property def");
		assert_eq!(p.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_expression_bodied_property_emits_property_def() {
		let src = "namespace Foo;\npublic class Bar {\n    public int N => 42;\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		assert!(g.defs().any(|d| d.kind == b"property"
			&& d.moniker.as_view().segments().last().unwrap().name == b"N"));
	}

	#[test]
	fn extract_property_with_user_type_emits_uses_type() {
		let src = "namespace Foo;\npublic class Other {}\npublic class Bar {\n    public Other Item { get; set; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		assert!(g.refs().any(|r| r.kind == b"uses_type"
			&& r.target.as_view().segments().last().unwrap().name == b"Other"));
	}

	#[test]
	fn extract_base_list_emits_extends_per_entry() {
		let src = "namespace Foo;\npublic class Base {}\npublic class Foo : Base, IBar {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let names: Vec<&[u8]> = g
			.refs()
			.filter(|r| r.kind == b"extends")
			.map(|r| r.target.as_view().segments().last().unwrap().name)
			.collect();
		assert!(names.contains(&&b"Base"[..]));
		assert!(names.contains(&&b"IBar"[..]));
	}

	#[test]
	fn extract_generic_base_emits_extends_on_head_and_uses_type_on_arg() {
		let src = "namespace Foo;\npublic class List<T> {}\npublic class Bar : List<int> {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		assert!(g.refs().any(|r| r.kind == b"extends"
			&& r.target.as_view().segments().last().unwrap().name == b"List"));
	}

	#[test]
	fn extract_interface_base_emits_extends_per_entry() {
		let src = "namespace Foo;\npublic interface IFoo : IBar, IBaz {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let count = g.refs().filter(|r| r.kind == b"extends").count();
		assert_eq!(count, 2);
	}

	#[test]
	fn extract_method_param_user_type_emits_uses_type() {
		let src = "namespace Foo;\npublic class Other {}\npublic class Bar {\n    public void Take(Other o) {}\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		assert!(
			g.refs().any(|r| r.kind == b"uses_type"
				&& r.target.as_view().segments().last().unwrap().name == b"Other"),
			"refs: {:?}",
			g.refs().map(|r| r.kind.clone()).collect::<Vec<_>>()
		);
	}

	#[test]
	fn extract_using_simple_emits_imports_module_external() {
		let g = extract_default("F.cs", "using System;\n", &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		assert_eq!(r.confidence, b"external".to_vec());
		let target = MonikerBuilder::new()
			.project(b"app")
			.segment(b"external_pkg", b"System")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_using_dotted_path_segments() {
		let g = extract_default(
			"F.cs",
			"using System.Collections.Generic;\n",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		let names: Vec<&[u8]> = r.target.as_view().segments().map(|s| s.name).collect();
		assert_eq!(
			names,
			vec![&b"System"[..], &b"Collections"[..], &b"Generic"[..]]
		);
	}

	#[test]
	fn extract_using_third_party_marks_imported() {
		let g = extract_default("F.cs", "using Newtonsoft.Json;\n", &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		assert_eq!(r.confidence, b"imported".to_vec());
	}

	#[test]
	fn extract_using_alias_records_alias_attr() {
		let g = extract_default("F.cs", "using IO = System.IO;\n", &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		assert_eq!(r.alias, b"IO".to_vec());
	}

	#[test]
	fn extract_global_using_emits_imports_module() {
		let g = extract_default("F.cs", "global using System;\n", &make_anchor(), false);
		assert!(
			g.refs()
				.any(|r| r.kind == b"imports_module" && r.confidence == b"external".to_vec())
		);
	}

	#[test]
	fn extract_using_static_emits_imports_module() {
		let g = extract_default("F.cs", "using static System.Math;\n", &make_anchor(), false);
		assert!(g.refs().any(|r| r.kind == b"imports_module"));
	}

	#[test]
	fn extract_simple_invocation_to_unresolved_callee_uses_name_only() {
		let src = "class B {\n    void M() { Helper(1, 2); }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"Helper"
			})
			.expect("calls Helper (name-only)");
		assert_eq!(r.confidence, b"name_match".to_vec());
	}

	#[test]
	fn extract_simple_invocation_to_same_module_resolves_slots() {
		let src = "class B {\n    void M() { Helper(1); }\n    void Helper(int n) {}\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"Helper(n:int)"
			})
			.expect("calls Helper(n:int)");
		assert_eq!(r.confidence, b"name_match".to_vec());
	}

	#[test]
	fn extract_member_invocation_emits_method_call_with_receiver_hint() {
		let src = "class B {\n    void M() { Console.WriteLine(\"hi\"); }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("method_call ref");
		assert_eq!(r.receiver_hint, b"Console".to_vec());
		assert_eq!(
			r.target.as_view().segments().last().unwrap().name,
			b"WriteLine"
		);
	}

	#[test]
	fn extract_chained_member_call_receiver_hint_is_call() {
		let src = "class B {\n    void M() { foo().bar(); }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"bar"
			})
			.expect("method_call bar");
		assert_eq!(r.receiver_hint, b"call".to_vec());
	}

	#[test]
	fn extract_object_creation_emits_instantiates() {
		let src = "namespace Foo;\npublic class Bar {}\nclass C {\n    void M() { var x = new Bar(); }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates")
			.expect("instantiates ref");
		assert_eq!(r.target.as_view().segments().last().unwrap().name, b"Bar");
		assert_eq!(r.confidence, b"resolved".to_vec());
	}

	#[test]
	fn extract_object_creation_unresolved_marks_name_match() {
		let src = "class C {\n    void M() { var x = new Unknown(); }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates")
			.expect("instantiates ref");
		assert_eq!(r.confidence, b"name_match".to_vec());
	}

	#[test]
	fn extract_invocation_visits_arguments_for_nested_calls() {
		let src = "class B {\n    void M() { Outer(Inner()); }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let names: Vec<&[u8]> = g
			.refs()
			.filter(|r| r.kind == b"calls")
			.map(|r| r.target.as_view().segments().last().unwrap().name)
			.collect();
		assert!(names.contains(&&b"Outer"[..]));
		assert!(names.contains(&&b"Inner"[..]));
	}

	#[test]
	fn extract_class_attribute_emits_annotates() {
		let src = "namespace Foo;\n[Serializable]\npublic class Bar {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"annotates")
			.expect("annotates ref");
		assert_eq!(
			r.target.as_view().segments().last().unwrap().name,
			b"Serializable"
		);
	}

	#[test]
	fn extract_method_attribute_emits_annotates() {
		let src = "namespace Foo;\npublic class Bar {\n    [HttpGet] public void M() {}\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"annotates")
			.expect("annotates ref");
		assert_eq!(
			r.target.as_view().segments().last().unwrap().name,
			b"HttpGet"
		);
	}

	#[test]
	fn extract_multiple_attribute_lists_each_emit_annotates() {
		let src =
			"namespace Foo;\npublic class Bar {\n    [Required] [Range(1,9)] public int N;\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let names: Vec<&[u8]> = g
			.refs()
			.filter(|r| r.kind == b"annotates")
			.map(|r| r.target.as_view().segments().last().unwrap().name)
			.collect();
		assert!(names.contains(&&b"Required"[..]));
		assert!(names.contains(&&b"Range"[..]));
	}

	#[test]
	fn extract_qualified_attribute_resolves_leaf_name() {
		let src = "namespace Foo;\n[System.Serializable]\npublic class Bar {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		assert!(g.refs().any(|r| r.kind == b"annotates"
			&& r.target.as_view().segments().last().unwrap().name == b"Serializable"));
	}

	#[test]
	fn extract_shallow_skips_param_and_local_defs() {
		let src = "class B {\n    void M(int x) { int y = 1; var z = \"\"; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		assert!(
			g.defs().all(|d| d.kind != b"param" && d.kind != b"local"),
			"shallow extraction must not emit param/local defs"
		);
	}

	#[test]
	fn extract_deep_emits_param_def() {
		let src = "class B {\n    void M(int a, string b) {}\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), true);
		let pa = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"module", b"F")
			.segment(b"class", b"B")
			.segment(b"method", b"M(a:int,b:string)")
			.segment(b"param", b"a")
			.build();
		assert!(g.contains(&pa));
	}

	#[test]
	fn extract_deep_emits_local_def_for_typed_var() {
		let src = "class B {\n    void M() { int x = 5; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), true);
		let lx = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"module", b"F")
			.segment(b"class", b"B")
			.segment(b"method", b"M()")
			.segment(b"local", b"x")
			.build();
		assert!(g.contains(&lx));
	}

	#[test]
	fn extract_deep_emits_local_def_for_implicit_var() {
		let src = "class B {\n    void M() { var s = \"hi\"; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), true);
		assert!(
			g.defs().any(|d| d.kind == b"local"
				&& d.moniker.as_view().segments().last().unwrap().name == b"s")
		);
	}

	#[test]
	fn extract_deep_emits_local_def_for_foreach_iter() {
		let src = "class B {\n    void M(int[] items) { foreach (var item in items) {} }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), true);
		assert!(g.defs().any(|d| d.kind == b"local"
			&& d.moniker.as_view().segments().last().unwrap().name == b"item"));
	}

	#[test]
	fn extract_deep_skips_blank_local() {
		let src = "class B {\n    void M() { var _ = 1; var y = 2; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), true);
		let names: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"local")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert_eq!(names, vec![&b"y"[..]]);
	}
}
