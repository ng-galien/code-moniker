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

use canonicalize::{compute_module_moniker, read_package_name};
use walker::{Walker, collect_type_table};

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
	let mut graph = CodeGraph::new(module.clone(), kinds::MODULE);
	let mut type_table: HashMap<&[u8], Moniker> = HashMap::new();
	collect_type_table(
		tree.root_node(),
		source.as_bytes(),
		&module,
		&mut type_table,
	);
	let walker = Walker {
		source_bytes: source.as_bytes(),
		module: module.clone(),
		deep,
		presets,
		local_scope: RefCell::new(Vec::new()),
		imports: RefCell::new(HashMap::<&[u8], &'static [u8]>::new()),
		type_table,
	};
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

pub struct Lang;

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "java";
	const ALLOWED_KINDS: &'static [&'static str] = &[
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
	fn extract_module_uses_package_decl_and_class_filename() {
		let src = "package com.acme;\nclass Foo {}\n";
		let g = extract_default("src/Foo.java", src, &make_anchor(), false);
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"java")
			.segment(b"package", b"com")
			.segment(b"package", b"acme")
			.segment(b"module", b"Foo")
			.build();
		assert_eq!(g.root(), &expected);
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
	fn extract_class_with_public_modifier_carries_visibility_public() {
		let g = extract_default("Foo.java", "public class Foo {}", &make_anchor(), false);
		let foo = g.defs().find(|d| d.kind == b"class").unwrap();
		assert_eq!(foo.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_method_signature_in_moniker_and_signature_column() {
		let src = r#"
            public class Foo {
                public int findById(int id, String name) { return id; }
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		let last = m.moniker.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"method");
		assert_eq!(last.name, b"findById(int,String)");
		assert_eq!(m.signature, b"int,String".to_vec());
		assert_eq!(m.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_constructor_uses_constructor_kind() {
		let src = r#"
            public class Foo {
                public Foo(int x) {}
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		assert!(g.defs().any(|d| d.kind == b"constructor"));
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
	fn extract_record_emits_record_def() {
		let g = extract_default(
			"Point.java",
			"public record Point(int x, int y) {}",
			&make_anchor(),
			false,
		);
		let pt = g.defs().find(|d| d.kind == b"record").expect("record def");
		assert_eq!(pt.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_extends_and_implements_emit_refs() {
		let src = r#"
            public class A extends B implements I, J {}
        "#;
		let g = extract_default("A.java", src, &make_anchor(), false);
		let kinds: Vec<&[u8]> = g.refs().map(|r| r.kind.as_slice()).collect();
		assert_eq!(kinds.iter().filter(|k| **k == b"extends").count(), 1);
		assert_eq!(kinds.iter().filter(|k| **k == b"implements").count(), 2);
	}

	#[test]
	fn extract_named_jdk_import_marks_external() {
		let src = "import java.util.List;\nclass Foo {}";
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_symbol")
			.expect("imports_symbol ref");
		assert_eq!(r.confidence, b"external".to_vec());
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
	fn extract_method_call_receiver_hint_carries_identifier_text() {
		let src = r#"
            class Foo {
                void m() { obj.bar(); }
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("method_call ref");
		assert_eq!(
			r.receiver_hint,
			b"obj".to_vec(),
			"receiver hint must carry the local identifier text",
		);
	}

	#[test]
	fn extract_object_creation_emits_instantiates() {
		let src = r#"
            class Foo {
                Object m() { return new Bar(); }
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates")
			.expect("instantiates ref");
		let last = r.target.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"class");
		assert_eq!(last.name, b"Bar");
	}

	#[test]
	fn extract_annotation_on_class_emits_annotates() {
		let src = "@Deprecated public class Foo {}";
		let g = extract_default("Foo.java", src, &make_anchor(), false);
		assert!(g.refs().any(|r| r.kind == b"annotates"));
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
	fn extract_same_file_type_resolves_with_real_target() {
		let src = r#"
            class Bar {}
            class Foo {
                Bar b;
                Object m() { return new Bar(); }
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), false);

		let bar_def = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"java")
			.segment(b"module", b"Foo")
			.segment(b"class", b"Bar")
			.build();

		let uses = g
			.refs()
			.find(|r| r.kind == b"uses_type" && r.target == bar_def)
			.expect("uses_type ref to Bar");
		assert_eq!(uses.confidence, b"resolved".to_vec());

		let inst = g
			.refs()
			.find(|r| r.kind == b"instantiates" && r.target == bar_def)
			.expect("instantiates ref to Bar");
		assert_eq!(inst.confidence, b"resolved".to_vec());
	}

	#[test]
	fn extract_nested_type_resolves_via_table() {
		let src = r#"
            class Outer {
                static class Inner {}
                Inner make() { return new Inner(); }
            }
        "#;
		let g = extract_default("Outer.java", src, &make_anchor(), false);
		let inner = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"java")
			.segment(b"module", b"Outer")
			.segment(b"class", b"Outer")
			.segment(b"class", b"Inner")
			.build();
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates" && r.target == inner)
			.expect("instantiates Inner");
		assert_eq!(r.confidence, b"resolved".to_vec());
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

	#[test]
	fn extract_lambda_param_marks_reads_as_local() {
		let src = r#"
            class Foo {
                java.util.function.BinaryOperator<Integer> add = (a, b) -> a + b;
            }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), true);
		let read_a = g
			.refs()
			.find(|r| {
				r.kind == b"reads" && r.target.as_view().segments().last().unwrap().name == b"a"
			})
			.expect("reads a inside lambda");
		assert_eq!(read_a.confidence, b"local".to_vec());
	}

	#[test]
	fn extract_param_read_marks_confidence_local() {
		let src = r#"
            class Foo { int m(int x) { return x; } }
        "#;
		let g = extract_default("Foo.java", src, &make_anchor(), true);
		let r = g.refs().find(|r| r.kind == b"reads").expect("reads ref");
		assert_eq!(r.confidence, b"local".to_vec());
	}
}
