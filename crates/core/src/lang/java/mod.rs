use std::cell::RefCell;
use std::collections::HashMap;

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

use canonicalize::{compute_module_moniker, read_package_name};
use strategy::{CallableTable, Strategy, collect_callable_table, collect_type_table};

#[derive(Clone, Debug, Default)]
pub struct Presets {
	pub external_packages: Vec<String>,
}

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_java::LANGUAGE.into();
	parser
		.set_language(&language)
		.expect("failed to load tree-sitter Java grammar");
	parser
		.parse(source, None)
		.expect("tree-sitter parse returned None on a non-cancelled call")
}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let tree = parse(source);
	let pkg = read_package_name(tree.root_node(), source.as_bytes());
	let pieces: Vec<&str> = pkg.split('.').filter(|s| !s.is_empty()).collect();
	let module = compute_module_moniker(anchor, uri, &pieces);
	let (def_cap, ref_cap) = CodeGraph::capacity_for_source(source.len());
	let mut graph = CodeGraph::with_capacity(module.clone(), kinds::MODULE, def_cap, ref_cap);
	let mut type_table: HashMap<&[u8], Moniker> = HashMap::new();
	collect_type_table(
		tree.root_node(),
		source.as_bytes(),
		&module,
		&mut type_table,
	);
	let mut callable_table: CallableTable = HashMap::new();
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
		presets,
		imports: RefCell::new(HashMap::<Vec<u8>, &'static [u8]>::new()),
		import_targets: RefCell::new(HashMap::<Vec<u8>, _>::new()),
		local_scope: RefCell::new(Vec::new()),
		type_table,
		callable_table,
	};
	let walker = CanonicalWalker::new(&strat, source.as_bytes());
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

pub struct Lang;

const DEF_KINDS: &[&str] = &[
	"class",
	"interface",
	"enum",
	"record",
	"annotation_type",
	"method",
	"constructor",
	"field",
	"enum_constant",
];

const DEF_KIND_SPECS: &[KindSpec] = &[
	KindSpec::new("class", Shape::Type, 20, "class"),
	KindSpec::new("interface", Shape::Type, 21, "interface"),
	KindSpec::new("enum", Shape::Type, 22, "enum"),
	KindSpec::new("record", Shape::Type, 23, "record"),
	KindSpec::new("annotation_type", Shape::Type, 24, "annotation_type"),
	KindSpec::new("enum_constant", Shape::Value, 30, "enum_constant"),
	KindSpec::new("field", Shape::Value, 31, "field"),
	KindSpec::new("constructor", Shape::Callable, 40, "constructor"),
	KindSpec::new("method", Shape::Callable, 41, "method"),
];

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "java";
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
	fn parse_empty_returns_program() {
		let tree = parse("");
		assert_eq!(tree.root_node().kind(), "program");
	}

	#[test]
	fn extract_default_package_skips_package_segments() {
		let g = extract_default("Foo.java", "class Foo {}", &make_anchor(), false);
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"java")
			.segment(b"module", b"Foo")
			.build();
		assert_eq!(g.root(), &expected);
	}

	#[test]
	fn extract_class_emits_class_def_with_package_visibility_default() {
		let g = extract_default("Foo.java", "class Foo {}", &make_anchor(), false);
		let foo = g.defs().find(|d| d.kind == b"class").expect("class def");
		assert_eq!(foo.visibility, b"package".to_vec());
	}

	#[test]
	fn extract_field_one_def_per_declarator() {
		let src = "class Foo { int a, b; private String name; }";
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let fields: Vec<_> = g.defs().filter(|d| d.kind == b"field").collect();
		assert_eq!(
			fields.len(),
			3,
			"got {:?}",
			fields.iter().map(|d| &d.moniker).collect::<Vec<_>>()
		);
		let private_field = fields
			.iter()
			.find(|d| d.moniker.as_view().segments().last().unwrap().name == b"name")
			.unwrap();
		assert_eq!(private_field.visibility, b"private".to_vec());
	}

	#[test]
	fn extract_record_components_emit_fields_and_accessors() {
		let g = extract_default(
			"User.java",
			"public record User(String id, int age) {}",
			&make_anchor(),
			false,
		);
		let user = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"java")
			.segment(b"module", b"User")
			.segment(b"record", b"User")
			.build();
		let user_idx = g
			.defs()
			.enumerate()
			.find_map(|(idx, def)| (def.moniker == user).then_some(idx))
			.expect("record def index");
		for name in [b"id".as_slice(), b"age".as_slice()] {
			let field = MonikerBuilder::from_view(user.as_view())
				.segment(b"field", name)
				.build();
			let accessor_name = [name, b"()"].concat();
			let accessor = MonikerBuilder::from_view(user.as_view())
				.segment(b"method", &accessor_name)
				.build();

			let field_def = g
				.defs()
				.find(|d| d.moniker == field)
				.unwrap_or_else(|| panic!("missing record component field {name:?}"));
			assert_eq!(field_def.visibility, b"private".to_vec());
			let accessor_def = g
				.defs()
				.find(|d| d.moniker == accessor)
				.unwrap_or_else(|| panic!("missing record accessor {name:?}"));
			assert_eq!(accessor_def.visibility, b"public".to_vec());
			assert_eq!(accessor_def.signature, b"".to_vec());
		}
		assert!(
			g.refs().any(|r| r.kind == b"uses_type"
				&& r.source == user_idx
				&& r.target.as_view().segments().last().unwrap().name == b"String"),
			"record component type should emit a uses_type ref"
		);
	}

	#[test]
	fn extract_record_component_keeps_explicit_accessor_without_duplicate() {
		let g = extract_default(
			"User.java",
			"public record User(String id) { public String id() { return id; } }",
			&make_anchor(),
			false,
		);
		let accessors: Vec<_> = g
			.defs()
			.filter(|d| {
				d.kind == b"method"
					&& d.moniker.as_view().segments().last().unwrap().name == b"id()"
			})
			.collect();
		assert_eq!(
			accessors.len(),
			1,
			"record accessor should be emitted once: {:?}",
			accessors.iter().map(|d| &d.moniker).collect::<Vec<_>>()
		);
		assert!(
			g.defs().any(|d| d.kind == b"field"
				&& d.moniker.as_view().segments().last().unwrap().name == b"id")
		);
	}

	#[test]
	fn record_zero_arg_accessor_resolution_survives_same_name_overload() {
		let g = extract_default(
			"User.java",
			r#"public record User(String id) {
                String id(String prefix) { return prefix + id; }
                String current() { return this.id(); }
            }"#,
			&make_anchor(),
			false,
		);
		let target_names: Vec<_> = g
			.refs()
			.filter(|r| r.kind == b"method_call" && r.receiver_hint == b"this")
			.map(|r| r.target.as_view().segments().last().unwrap().name.to_vec())
			.collect();
		assert!(
			target_names.iter().any(|name| name == b"id()"),
			"this.id() should resolve to the zero-arg record accessor, got {target_names:?}"
		);
		assert!(
			!target_names.iter().any(|name| name == b"id(prefix:String)"),
			"this.id() must not resolve to same-name overload, got {target_names:?}"
		);
	}

	#[test]
	fn this_call_arity_mismatch_does_not_resolve_to_only_overload() {
		let g = extract_default(
			"User.java",
			r#"class User {
                String id(String prefix) { return prefix; }
                String current() { return this.id(); }
            }"#,
			&make_anchor(),
			false,
		);
		let target_names: Vec<_> = g
			.refs()
			.filter(|r| r.kind == b"method_call" && r.receiver_hint == b"this")
			.map(|r| r.target.as_view().segments().last().unwrap().name.to_vec())
			.collect();

		assert!(
			!target_names.iter().any(|name| name == b"id(prefix:String)"),
			"this.id() must not resolve to one-arg overload, got {target_names:?}"
		);
		assert!(
			target_names.iter().any(|name| name == b"id"),
			"this.id() should remain unresolved/name-only on arity mismatch, got {target_names:?}"
		);
	}

	#[test]
	fn extract_enum_emits_enum_constants() {
		let g = extract_default(
			"Color.java",
			"public enum Color { RED, GREEN }",
			&make_anchor(),
			false,
		);
		let red = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"java")
			.segment(b"module", b"Color")
			.segment(b"enum", b"Color")
			.segment(b"enum_constant", b"RED")
			.build();
		assert!(
			g.contains(&red),
			"missing RED, defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_wildcard_import_emits_imports_module() {
		let src = "import com.acme.*;\nclass Foo {}";
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module ref");
		assert_eq!(r.confidence, b"imported".to_vec());
	}

	#[test]
	fn extract_method_call_carries_receiver_hint() {
		let src = r#"
            class Foo {
                void m() { this.bar(); }
                void bar() {}
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("method_call ref");
		assert_eq!(r.receiver_hint, b"this".to_vec());
	}

	#[test]
	fn method_call_on_imported_class_carries_imported_confidence() {
		let src = r#"
            import com.acme.Util;
            class Foo {
                void m() { Util.run(); }
            }
        "#;
		let g = extract_default("src/Foo.java", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call" && r.receiver_hint == b"Util")
			.expect("method_call on Util");
		assert_eq!(r.confidence, b"imported");
	}

	#[test]
	fn method_call_on_non_imported_identifier_stays_name_match() {
		let src = r#"
            class Foo {
                void m() { obj.bar(); }
            }
        "#;
		let g = extract_default("src/Foo.java", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call" && r.receiver_hint == b"obj")
			.expect("method_call on obj");
		assert_eq!(r.confidence, b"name_match");
	}

	#[test]
	fn this_call_resolves_to_full_slot_signature() {
		let src = r#"
            class Foo {
                void m() { this.bar(); }
                void bar() {}
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("method_call ref");
		let last = r.target.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"method");
		assert_eq!(
			last.name, b"bar()",
			"this.bar() must resolve to the def's slot signature, not to a name-only fallback"
		);
	}

	#[test]
	fn method_call_on_unresolved_receiver_falls_back_to_name_only() {
		let src = r#"
            class Foo {
                void m() { obj.bar(1); }
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("method_call ref");
		let last = r.target.as_view().segments().last().unwrap();
		assert_eq!(
			last.name, b"bar",
			"unresolved receiver must produce a name-only target (no parens, no arity)"
		);
	}

	#[test]
	fn extract_imported_call_marks_confidence_imported() {
		let src = r#"
            import com.acme.Helpers;
            class Foo { void m() { Helpers.go(); } }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let reads_helpers = g.refs().find(|r| {
			r.kind == b"reads" && r.target.as_view().segments().last().unwrap().name == b"Helpers"
		});
		if let Some(r) = reads_helpers {
			assert_eq!(r.confidence, b"imported".to_vec());
		}
	}

	#[test]
	fn extract_deep_catch_param_emits_local_def() {
		let src = r#"
            class Foo {
                void m() { try {} catch (IOException e) { e.toString(); } }
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), true);
		let monikers = g.def_monikers();
		let e = monikers.iter().find(|m| {
			let last = m.as_view().segments().last().unwrap();
			last.kind == b"param" && last.name == b"e"
		});
		assert!(
			e.is_some(),
			"catch param should be emitted as a param def in deep mode"
		);
	}

	#[test]
	fn extract_deep_enhanced_for_var_is_local() {
		let src = r#"
            class Foo {
                void m(java.util.List<String> xs) { for (String x : xs) { x.length(); } }
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), true);
		assert!(
			g.defs().any(|d| d.kind == b"local"
				&& d.moniker.as_view().segments().last().unwrap().name == b"x"),
			"enhanced-for var should be a local def"
		);
	}
}
