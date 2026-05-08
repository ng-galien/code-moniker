use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

pub mod build;
mod canonicalize;
mod imports;
mod kinds;
mod refs;
mod scope;
mod walker;

use canonicalize::compute_module_moniker;
use scope::collect_export_ranges;
use walker::Walker;

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
	parser
		.set_language(&language)
		.expect("failed to load tree-sitter TypeScript grammar");
	parser
		.parse(source, None)
		.expect("tree-sitter parse returned None on a non-cancelled call")
}

#[derive(Clone, Debug, Default)]
pub struct Presets {
	pub di_register_callees: Vec<String>,
}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let mut graph = CodeGraph::new(module.clone(), kinds::MODULE);
	let tree = parse(source);
	let export_ranges = collect_export_ranges(tree.root_node());
	let walker = Walker {
		source_bytes: source.as_bytes(),
		module: module.clone(),
		deep,
		presets,
		export_ranges,
		local_scope: std::cell::RefCell::new(Vec::new()),
	};
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::code_graph::assert_local_refs_closed;
	use crate::core::moniker::MonikerBuilder;

	fn extract(uri: &str, source: &str, anchor: &Moniker, deep: bool) -> CodeGraph {
		let g = super::extract(uri, source, anchor, deep, &Presets::default());
		assert_local_refs_closed(&g);
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
	fn extract_empty_source_yields_module_only_graph() {
		let anchor = make_anchor();
		let graph = extract("src/lib/util.ts", "", &anchor, false);
		assert_eq!(graph.def_count(), 1);
		assert_eq!(graph.ref_count(), 0);

		let expected = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"dir", b"src")
			.segment(b"dir", b"lib")
			.segment(b"module", b"util")
			.build();
		assert_eq!(graph.root(), &expected);
	}

	#[test]
	fn extract_strips_each_known_extension() {
		let anchor = make_anchor();
		for uri in ["foo.ts", "foo.tsx", "foo.js", "foo.jsx", "foo.mjs", "foo.cjs"] {
			let g = extract(uri, "", &anchor, false);
			let last = g.root().as_view().segments().last().unwrap();
			assert_eq!(last.name, b"foo", "extension not stripped on {uri}");
		}
	}

	#[test]
	fn extract_simple_class_emits_class_def() {
		let anchor = make_anchor();
		let graph = extract("util.ts", "class Foo {}", &anchor, false);
		assert_eq!(graph.def_count(), 2);

		let foo = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.build();
		assert!(graph.contains(&foo));
	}

	#[test]
	fn extract_export_class_descends_into_export_statement() {
		let anchor = make_anchor();
		let graph = extract("util.ts", "export class Foo {}", &anchor, false);
		assert_eq!(graph.def_count(), 2);
	}

	#[test]
	fn extract_class_with_method_emits_method_def() {
		let anchor = make_anchor();
		let graph = extract("util.ts", "class Foo { bar() {} }", &anchor, false);
		assert_eq!(graph.def_count(), 3);

		let bar = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar()")
			.build();
		assert!(graph.contains(&bar));
	}

	#[test]
	fn extract_function_declaration_emits_def() {
		let anchor = make_anchor();
		let graph = extract("util.ts", "function foo() {}", &anchor, false);
		assert_eq!(graph.def_count(), 2);

		let foo = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"foo()")
			.build();
		assert!(graph.contains(&foo));
	}
	#[test]
	fn extract_named_import_emits_imports_symbol_per_specifier() {
		let g = extract(
			"src/util.ts",
			"import { Bar, Baz } from './bar';",
			&make_anchor(),
			false,
		);
		let kinds: Vec<_> = g.refs().map(|r| r.kind.clone()).collect();
		assert_eq!(kinds.len(), 2, "one ref per named specifier; got {kinds:?}");
		assert!(kinds.iter().all(|k| k == b"imports_symbol"));

		let bar = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"dir", b"src")
			.segment(b"module", b"bar")
			.segment(b"path", b"Bar")
			.build();
		let baz = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"dir", b"src")
			.segment(b"module", b"bar")
			.segment(b"path", b"Baz")
			.build();
		let targets: Vec<_> = g.refs().map(|r| r.target.clone()).collect();
		assert!(targets.contains(&bar), "missing Bar target: {targets:?}");
		assert!(targets.contains(&baz));
	}

	#[test]
	fn extract_default_import_emits_imports_symbol_default() {
		let g = extract("util.ts", "import Foo from './foo';", &make_anchor(), false);
		let r = g.refs().next().expect("one ref");
		assert_eq!(r.kind, b"imports_symbol".to_vec());
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"foo")
			.segment(b"path", b"default")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_namespace_import_emits_imports_module() {
		let g = extract("util.ts", "import * as M from './foo';", &make_anchor(), false);
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"imports_module".to_vec());
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"foo")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_bare_import_resolves_to_external_pkg() {
		let g = extract("util.ts", "import { useState } from 'react';", &make_anchor(), false);
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"imports_symbol".to_vec());
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"external_pkg", b"react")
			.segment(b"path", b"useState")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_scoped_bare_import_keeps_full_scope() {
		let g = extract(
			"util.ts",
			"import { join } from '@scope/pkg/sub';",
			&make_anchor(),
			false,
		);
		let r = g.refs().next().unwrap();
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"external_pkg", b"@scope/pkg")
			.segment(b"path", b"sub")
			.segment(b"path", b"join")
			.build();
		assert_eq!(r.target, target);
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
	fn extract_side_effect_import_emits_imports_module() {
		let g = extract("util.ts", "import 'side-effects';", &make_anchor(), false);
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"imports_module".to_vec());
	}
	#[test]
	fn extract_named_reexport_emits_reexports_per_specifier() {
		let g = extract(
			"index.ts",
			"export { Foo, Bar } from './lib';",
			&make_anchor(),
			false,
		);
		let kinds: Vec<_> = g.refs().map(|r| r.kind.clone()).collect();
		assert_eq!(kinds.len(), 2);
		assert!(kinds.iter().all(|k| k == b"reexports"));
	}

	#[test]
	fn extract_star_reexport_emits_single_reexports_ref() {
		let g = extract("index.ts", "export * from './lib';", &make_anchor(), false);
		assert_eq!(g.ref_count(), 1);
		let r = g.refs().next().unwrap();
		assert_eq!(r.kind, b"reexports".to_vec());
	}
	#[test]
	fn extract_interface_emits_interface_def() {
		let g = extract("util.ts", "interface Greet { hi(): void; }", &make_anchor(), false);
		let greet = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"interface", b"Greet")
			.build();
		assert!(g.contains(&greet));
		let hi = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"interface", b"Greet")
			.segment(b"method", b"hi()")
			.build();
		assert!(g.contains(&hi), "method_signature in interface body must be a method def");
	}

	#[test]
	fn extract_enum_emits_enum_constants() {
		let g = extract("util.ts", "enum Color { Red, Green = 1 }", &make_anchor(), false);
		let red = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"enum", b"Color")
			.segment(b"enum_constant", b"Red")
			.build();
		assert!(g.contains(&red), "missing Red enum constant; defs: {:?}", g.def_monikers());
	}

	#[test]
	fn extract_type_alias_emits_type_alias_def() {
		let g = extract("util.ts", "type Id = string;", &make_anchor(), false);
		let id = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"type_alias", b"Id")
			.build();
		assert!(g.contains(&id));
	}
	#[test]
	fn extract_method_signature_encoded_in_segment_name() {
		let g = extract(
			"util.ts",
			"class Foo { bar(a: number, b: string) {} }",
			&make_anchor(),
			false,
		);
		let bar = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar(number,string)")
			.build();
		assert!(g.contains(&bar), "expected typed segment, defs: {:?}", g.def_monikers());
	}

	#[test]
	fn extract_constructor_uses_constructor_kind() {
		let g = extract(
			"util.ts",
			"class Foo { constructor(x: number) {} }",
			&make_anchor(),
			false,
		);
		let ctor = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.segment(b"constructor", b"constructor(number)")
			.build();
		assert!(g.contains(&ctor));
	}

	#[test]
	fn extract_class_field_emits_field_def() {
		let g = extract(
			"util.ts",
			"class Foo { x: number = 0; }",
			&make_anchor(),
			false,
		);
		let x = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.segment(b"field", b"x")
			.build();
		assert!(g.contains(&x));
	}

	#[test]
	fn extract_module_const_emits_const_def() {
		let g = extract("util.ts", "const PI = 3.14;", &make_anchor(), false);
		let pi = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"const", b"PI")
			.build();
		assert!(g.contains(&pi));
	}

	#[test]
	fn extract_arrow_const_emits_function_def() {
		let g = extract(
			"util.ts",
			"const add = (a: number, b: number) => a + b;",
			&make_anchor(),
			false,
		);
		let add = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"add(number,number)")
			.build();
		assert!(
			g.contains(&add),
			"arrow-as-const must be a function def; defs: {:?}",
			g.def_monikers()
		);
	}
	#[test]
	fn extract_top_level_call_emits_calls_ref() {
		let g = extract("util.ts", "foo(1);", &make_anchor(), false);
		let r = g.refs().find(|r| r.kind == b"calls").expect("calls ref");
		assert_eq!(r.source, 0, "top-level call sources on the module");
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"foo(1)")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_visibility_module_for_unexported_class() {
		let g = extract("util.ts", "class Foo {}", &make_anchor(), false);
		let foo = g.defs().find(|d| d.kind == b"class").unwrap();
		assert_eq!(foo.visibility, b"module".to_vec());
	}

	#[test]
	fn extract_visibility_public_for_exported_class() {
		let g = extract("util.ts", "export class Foo {}", &make_anchor(), false);
		let foo = g.defs().find(|d| d.kind == b"class").unwrap();
		assert_eq!(foo.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_visibility_for_class_member_modifiers() {
		let g = extract(
			"util.ts",
			"export class C { public a() {}; protected b() {}; private c() {}; d() {} }",
			&make_anchor(),
			false,
		);
		let by_name = |n: &[u8]| {
			g.defs()
				.find(|d| d.moniker.as_view().segments().last().unwrap().name == n)
				.unwrap()
				.visibility
				.clone()
		};
		assert_eq!(by_name(b"a()"), b"public".to_vec());
		assert_eq!(by_name(b"b()"), b"protected".to_vec());
		assert_eq!(by_name(b"c()"), b"private".to_vec());
		assert_eq!(by_name(b"d()"), b"public".to_vec(), "no modifier defaults to public");
	}

	#[test]
	fn extract_named_import_alias_recorded() {
		let g = extract(
			"util.ts",
			"import { X as Y } from './foo';",
			&make_anchor(),
			false,
		);
		let r = g.refs().next().unwrap();
		assert_eq!(r.alias, b"Y".to_vec());
	}

	#[test]
	fn extract_namespace_import_alias_recorded() {
		let g = extract(
			"util.ts",
			"import * as Mod from './foo';",
			&make_anchor(),
			false,
		);
		let r = g.refs().next().unwrap();
		assert_eq!(r.alias, b"Mod".to_vec());
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
	fn extract_reads_unbound_identifier_marks_name_match() {
		let g = extract(
			"util.ts",
			"function f() { return outsideVar; }",
			&make_anchor(),
			false,
		);
		let r = g.refs().find(|r| r.kind == b"reads").unwrap();
		assert_eq!(r.confidence, b"name_match".to_vec());
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
		let g = extract(
			"util.ts",
			"function f(x) {}",
			&make_anchor(),
			true,
		);
		let p = g.defs().find(|d| d.kind == b"param").expect("param def");
		assert!(p.visibility.is_empty());
	}

	#[test]
	fn extract_import_confidence_distinguishes_relative_vs_external() {
		let g = extract(
			"util.ts",
			"import { a } from './local';\nimport { b } from 'react';",
			&make_anchor(),
			false,
		);
		let confs: Vec<&[u8]> = g.refs().map(|r| r.confidence.as_slice()).collect();
		assert!(confs.contains(&b"imported".as_slice()));
		assert!(confs.contains(&b"external".as_slice()));
	}

	#[test]
	fn extract_method_call_carries_receiver_hint() {
		let cases = [
			("class C { m() { this.bar(); } }", b"this".as_slice()),
			("class C { m() { super.bar(); } }", b"super".as_slice()),
			("obj.bar();", b"obj".as_slice()),
			("a.b.bar();", b"member".as_slice()),
			("foo().bar();", b"call".as_slice()),
		];
		for (src, expected) in cases {
			let g = extract("util.ts", src, &make_anchor(), false);
			let r = g
				.refs()
				.find(|r| r.kind == b"method_call")
				.unwrap_or_else(|| panic!("no method_call ref for: {src}"));
			assert_eq!(
				r.receiver_hint.as_slice(),
				expected,
				"receiver hint mismatch for {src:?}"
			);
		}
	}

	#[test]
	fn extract_method_call_receiver_hint_carries_imported_alias() {
		let g = extract(
			"explorer.ts",
			"import { z } from 'zod';\nconst schema = z.string();",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("method_call ref");
		assert_eq!(
			r.receiver_hint.as_slice(),
			b"z",
			"receiver hint must carry the alias text so the consumer can join to imports_symbol",
		);
	}

	#[test]
	fn extract_method_call_emits_method_call_ref() {
		let g = extract("util.ts", "obj.bar(1, 2);", &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("method_call ref");
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"method", b"bar(2)")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_call_inside_method_sources_on_method() {
		let g = extract(
			"util.ts",
			"class C { m() { foo(); } }",
			&make_anchor(),
			false,
		);
		let r = g.refs().find(|r| r.kind == b"calls").expect("calls ref");
		let m_def = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"C")
			.segment(b"method", b"m()")
			.build();
		assert_eq!(g.defs().nth(r.source).unwrap().moniker, m_def);
	}

	#[test]
	fn extract_new_expression_emits_instantiates() {
		let g = extract("util.ts", "const x = new Foo();", &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates")
			.expect("instantiates ref");
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_class_extends_emits_extends_ref() {
		let g = extract(
			"util.ts",
			"class A extends B {}",
			&make_anchor(),
			false,
		);
		let r = g.refs().find(|r| r.kind == b"extends").expect("extends ref");
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"B")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_class_implements_emits_implements_ref() {
		let g = extract(
			"util.ts",
			"class A implements I {}",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"implements")
			.expect("implements ref");
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"interface", b"I")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_decorator_emits_annotates_ref() {
		let g = extract(
			"util.ts",
			"@Injectable class A {}",
			&make_anchor(),
			false,
		);
		let r = g
			.refs()
			.find(|r| r.kind == b"annotates")
			.expect("annotates ref");
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"Injectable()")
			.build();
		assert_eq!(r.target, target);
	}

	#[test]
	fn extract_decorator_call_keeps_arity() {
		let g = extract(
			"util.ts",
			"@Bind('x') class A {}",
			&make_anchor(),
			false,
		);
		let r = g.refs().find(|r| r.kind == b"annotates").unwrap();
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"Bind(1)")
			.build();
		assert_eq!(r.target, target);
	}
	#[test]
	fn extract_param_type_annotation_emits_uses_type() {
		let g = extract(
			"util.ts",
			"function f(x: Foo): Bar { return x as Bar; }",
			&make_anchor(),
			false,
		);
		let foo = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"Foo")
			.build();
		let bar = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"Bar")
			.build();
		let targets: Vec<_> = g
			.refs()
			.filter(|r| r.kind == b"uses_type")
			.map(|r| r.target.clone())
			.collect();
		assert!(targets.contains(&foo), "missing Foo uses_type; got {targets:?}");
		assert!(targets.contains(&bar));
	}

	#[test]
	fn extract_return_identifier_emits_reads() {
		let g = extract(
			"util.ts",
			"function f() { return x; }",
			&make_anchor(),
			false,
		);
		let r = g.refs().find(|r| r.kind == b"reads").expect("reads ref");
		let target = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"x()")
			.build();
		assert_eq!(r.target, target);
	}
	#[test]
	fn extract_di_register_fires_only_when_callee_in_preset() {
		let presets = Presets {
			di_register_callees: vec!["register".into(), "bind".into()],
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
		};
		let g = super::extract("util.ts", "expect(value);", &make_anchor(), false, &presets);
		assert!(g.refs().all(|r| r.kind != b"di_register"));
	}

	#[test]
	fn extract_di_register_register_with_name_and_factory() {
		let presets = Presets {
			di_register_callees: vec!["register".into()],
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
	fn extract_section_comment_emits_section_def() {
		let g = extract(
			"util.ts",
			"// ===== Public API =====\nclass Foo {}",
			&make_anchor(),
			false,
		);
		assert!(
			g.defs().any(|d| d.kind == b"section"),
			"expected section def; defs: {:?}",
			g.defs().map(|d| String::from_utf8_lossy(&d.kind).into_owned()).collect::<Vec<_>>()
		);
	}
	#[test]
	fn extract_export_default_class_named_default() {
		let g = extract(
			"util.ts",
			"export default class {}",
			&make_anchor(),
			false,
		);
		let m = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"class", b"default")
			.build();
		assert!(g.contains(&m));
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
			.segment(b"function", b"f(number,number)")
			.segment(b"param", b"a")
			.build();
		let pb = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"f(number,number)")
			.segment(b"param", b"b")
			.build();
		let sum = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"lang", b"ts")
			.segment(b"module", b"util")
			.segment(b"function", b"f(number,number)")
			.segment(b"local", b"sum")
			.build();
		assert!(g.contains(&pa), "missing param a; defs: {:?}", g.def_monikers());
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
	fn extract_position_covers_definition_node() {
		let g = extract("util.ts", "class Foo {}", &make_anchor(), false);
		let foo = g.defs().find(|d| d.kind == b"class").unwrap();
		let (s, e) = foo.position.unwrap();
		assert!(e > s);
	}
}
