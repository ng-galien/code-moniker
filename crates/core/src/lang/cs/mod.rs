use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;

use crate::lang::KindSpec;
pub mod build;
mod canonicalize;
mod kinds;
mod sdk_pipeline;

#[derive(Clone, Debug, Default)]
pub struct Presets {}

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_c_sharp::LANGUAGE.into();
	parser.set_language(&language).unwrap_or_else(|err| {
		panic!("failed to load tree-sitter C# grammar: {err}");
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
	_presets: &Presets,
) -> CodeGraph {
	sdk_pipeline::extract(uri, source, anchor, deep)
}

pub struct Lang;

const DEF_KINDS: &[&str] = &[
	"class",
	"interface",
	"struct",
	"record",
	"enum",
	"enum_constant",
	"delegate",
	"method",
	"constructor",
	"field",
	"property",
	"event",
];

const DEF_KIND_SPECS: &[KindSpec] = &[
	KindSpec::new("class", Shape::Type, 20, "class"),
	KindSpec::new("interface", Shape::Type, 21, "interface"),
	KindSpec::new("struct", Shape::Type, 22, "struct"),
	KindSpec::new("record", Shape::Type, 23, "record"),
	KindSpec::new("enum", Shape::Type, 24, "enum"),
	KindSpec::new("delegate", Shape::Type, 25, "delegate"),
	KindSpec::new("constructor", Shape::Callable, 40, "constructor"),
	KindSpec::new("method", Shape::Callable, 41, "method"),
	KindSpec::new("enum_constant", Shape::Value, 60, "enum_constant"),
	KindSpec::new("property", Shape::Value, 61, "property"),
	KindSpec::new("field", Shape::Value, 62, "field"),
	KindSpec::new("event", Shape::Value, 63, "event"),
];

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "cs";
	const ALLOWED_KINDS: &'static [&'static str] = DEF_KINDS;
	const KIND_SPECS: &'static [KindSpec] = DEF_KIND_SPECS;
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
	fn extract_struct_emits_struct_def() {
		let src = "namespace Foo;\npublic struct Bar {}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		assert!(g.defs().any(|d| d.kind == b"struct"
			&& d.moniker.as_view().segments().last().unwrap().name == b"Bar"));
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
	fn extract_enum_emits_enum_constants() {
		let src = "namespace Foo;\npublic enum Color { Red, Green }\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let red = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"module", b"F")
			.segment(b"enum", b"Color")
			.segment(b"enum_constant", b"Red")
			.build();
		let green = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"cs")
			.segment(b"module", b"F")
			.segment(b"enum", b"Color")
			.segment(b"enum_constant", b"Green")
			.build();
		assert!(g.contains(&red), "missing enum_constant:Red");
		assert!(g.contains(&green), "missing enum_constant:Green");
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
	fn extract_method_default_visibility_is_private() {
		let src = "namespace Foo;\npublic class Bar {\n    int Hidden() { return 0; }\n}\n";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(m.visibility, b"private".to_vec());
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
	fn extract_global_qualified_system_type_is_external() {
		let src = "class B { global::System.Resources.ResourceManager Resources { get; } }";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let reference = g
			.refs()
			.find(|reference| reference.kind == b"uses_type")
			.expect("uses_type reference");
		assert_eq!(reference.confidence, b"external".to_vec());
		let segments = reference.target.as_view().segments().collect::<Vec<_>>();
		assert!(
			segments
				.iter()
				.any(|segment| { segment.kind == b"sdk" && segment.name == b"cs" })
		);
		assert!(
			segments
				.iter()
				.any(|segment| { segment.kind == b"path" && segment.name == b"System" })
		);
		assert!(
			segments
				.iter()
				.any(|segment| { segment.kind == b"path" && segment.name == b"ResourceManager" })
		);
	}

	#[test]
	fn microsoft_namespace_is_not_implicitly_owned_by_the_sdk() {
		let src = "using Microsoft.Extensions.Logging; class Service {}";
		let g = extract_default("Service.cs", src, &make_anchor(), false);
		let reference = g
			.refs()
			.find(|reference| reference.kind == b"imports_module")
			.expect("using reference");
		let first = reference.target.as_view().segments().next().unwrap();
		assert_eq!(first.kind, b"external_pkg");
		assert_eq!(first.name, b"Microsoft");
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
	fn extract_using_third_party_marks_imported() {
		let g = extract_default("F.cs", "using Newtonsoft.Json;\n", &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		assert_eq!(r.confidence, b"imported".to_vec());
	}

	#[test]
	fn extract_system_prefixed_nuget_namespace_stays_manifest_owned() {
		for namespace in [
			"System.Reactive.Linq",
			"System.CommandLine",
			"System.IO.Abstractions",
			"System.Linq.Dynamic.Core",
		] {
			let source = format!("using {namespace};\n");
			let g = extract_default("F.cs", &source, &make_anchor(), false);
			let reference = g
				.refs()
				.find(|reference| reference.kind == b"imports_module")
				.expect("imports_module ref");
			assert_eq!(
				reference.confidence,
				b"imported".to_vec(),
				"{namespace} must be resolved through the project manifest",
			);
			assert_eq!(
				reference.target.as_view().segments().next().unwrap().kind,
				b"external_pkg",
			);
		}
	}

	#[test]
	fn extract_qualified_bcl_types_outside_the_short_catalog_use_sdk_targets() {
		let source = concat!(
			"class Service { ",
			"System.IO.StreamReader reader; ",
			"System.Net.Http.HttpClient client = new System.Net.Http.HttpClient(); ",
			"}",
		);
		let g = extract_default("Service.cs", source, &make_anchor(), false);
		for type_name in ["StreamReader", "HttpClient"] {
			let reference = g
				.refs()
				.find(|reference| {
					reference.kind == b"uses_type"
						&& reference
							.target
							.as_view()
							.segments()
							.last()
							.is_some_and(|segment| segment.name == type_name.as_bytes())
				})
				.unwrap_or_else(|| panic!("missing uses_type for {type_name}"));
			assert_eq!(
				reference.target.as_view().segments().next().unwrap().kind,
				b"sdk",
				"{type_name} must be owned by the target framework",
			);
			assert_eq!(reference.confidence, b"external".to_vec());
		}
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
		assert_eq!(r.call_name, b"Helper".to_vec());
		assert_eq!(r.call_arity, Some(2));
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
		assert_eq!(r.call_name, b"bar".to_vec());
		assert_eq!(r.call_arity, Some(0));
	}

	#[test]
	fn extract_callable_defs_expose_linkage_metadata() {
		let src = "class B { int Sum(int left, int right) => left + right; }";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let def = g
			.defs()
			.find(|def| def.kind == b"method")
			.expect("method definition");
		assert_eq!(def.call_name, b"Sum".to_vec());
		assert_eq!(def.call_arity, Some(2));
	}

	#[test]
	fn extract_callable_arity_comes_from_syntax_not_type_commas() {
		let src = "using System.Collections.Generic; class B { void One(Dictionary<string, int> value) {} void Many(params\nobject[] values) {} }";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let one = g
			.defs()
			.find(|def| def.call_name == b"One")
			.expect("One definition");
		let many = g
			.defs()
			.find(|def| def.call_name == b"Many")
			.expect("Many definition");
		assert_eq!(one.call_arity, Some(1));
		assert_eq!(many.call_arity, None);
	}

	#[test]
	fn extract_parameter_type_emits_typed_binding() {
		let src = "class Worker {} class B { void Run(Worker worker) {} }";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let reference = g
			.refs()
			.find(|reference| reference.kind == b"typed_as")
			.expect("typed_as reference");
		assert_eq!(reference.alias, b"worker".to_vec());
		assert_eq!(reference.confidence, b"resolved".to_vec());
	}

	#[test]
	fn extract_field_and_inferred_local_types_emit_typed_bindings() {
		let src =
			"class Worker {} class B { Worker worker; void Run() { var local = new Worker(); } }";
		let g = extract_default("F.cs", src, &make_anchor(), false);
		let typed = g
			.refs()
			.filter(|reference| reference.kind == b"typed_as")
			.collect::<Vec<_>>();
		assert!(typed.iter().any(|reference| {
			reference.alias.is_empty() && g.def_at(reference.source).kind == b"field"
		}));
		assert!(
			typed
				.iter()
				.any(|reference| reference.alias == b"local".to_vec())
		);
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
