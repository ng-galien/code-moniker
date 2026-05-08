//! Python parser and extractor.

use std::cell::RefCell;
use std::collections::HashMap;

use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

mod canonicalize;
mod kinds;
mod refs;
mod scope;
mod walker;

use canonicalize::compute_module_moniker;
use walker::{collect_type_table, Walker};

#[derive(Clone, Debug, Default)]
pub struct Presets {}

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_python::LANGUAGE.into();
	parser
		.set_language(&language)
		.expect("failed to load tree-sitter Python grammar");
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
	collect_type_table(tree.root_node(), source.as_bytes(), &module, &mut type_table);
	let walker = Walker {
		source_bytes: source.as_bytes(),
		module: module.clone(),
		deep,
		local_scope: RefCell::new(Vec::new()),
		imports: RefCell::new(HashMap::<&[u8], &'static [u8]>::new()),
		type_table,
	};
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::moniker::MonikerBuilder;

	fn make_anchor() -> Moniker {
		MonikerBuilder::new().project(b"app").build()
	}

	fn extract_default(uri: &str, source: &str, anchor: &Moniker, deep: bool) -> CodeGraph {
		extract(uri, source, anchor, deep, &Presets::default())
	}

	#[test]
	fn parse_empty_returns_module() {
		let tree = parse("");
		assert_eq!(tree.root_node().kind(), "module");
	}

	#[test]
	fn extract_module_uses_path_segments() {
		let g = extract_default("acme/util/text.py", "", &make_anchor(), false);
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"package", b"acme")
			.segment(b"package", b"util")
			.segment(b"module", b"text")
			.build();
		assert_eq!(g.root(), &expected);
	}

	#[test]
	fn extract_module_root_is_filename_only() {
		let g = extract_default("foo.py", "", &make_anchor(), false);
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"module", b"foo")
			.build();
		assert_eq!(g.root(), &expected);
	}

	#[test]
	fn extract_class_emits_class_def_with_public_visibility_default() {
		let g = extract_default("foo.py", "class Foo:\n    pass\n", &make_anchor(), false);
		let foo = g.defs().find(|d| d.kind == b"class").expect("class def");
		assert_eq!(foo.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_function_with_typed_params_emits_full_signature() {
		let src = "def make(x: int, y: str) -> int:\n    return x\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let f = g.defs().find(|d| d.kind == b"function").expect("function def");
		let last = f.moniker.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"function");
		assert_eq!(last.name, b"make(int,str)");
		assert_eq!(f.signature, b"int,str".to_vec());
	}

	#[test]
	fn extract_function_with_untyped_params_uses_underscore_placeholder() {
		let src = "def f(a, b=1):\n    return a\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let f = g.defs().find(|d| d.kind == b"function").expect("function def");
		let last = f.moniker.as_view().segments().last().unwrap();
		assert_eq!(last.name, b"f(_,_)");
		assert_eq!(f.signature, b"_,_".to_vec());
	}

	#[test]
	fn extract_method_excludes_self_from_signature() {
		let src = "class Foo:\n    def bar(self, x: int) -> int:\n        return x\n";
		let g = extract_default("foo.py", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		let last = m.moniker.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"method");
		assert_eq!(last.name, b"bar(int)");
		assert_eq!(m.signature, b"int".to_vec());
	}

	#[test]
	fn extract_classmethod_excludes_cls_from_signature() {
		let src = "class Foo:\n    @classmethod\n    def make(cls, x: int) -> 'Foo':\n        return cls()\n";
		let g = extract_default("foo.py", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(m.moniker.as_view().segments().last().unwrap().name, b"make(int)");
	}

	#[test]
	fn extract_dunder_visibility_is_public() {
		let src = "class Foo:\n    def __init__(self):\n        pass\n";
		let g = extract_default("foo.py", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("__init__");
		assert_eq!(m.visibility, b"public".to_vec());
	}

	#[test]
	fn extract_double_underscore_visibility_is_private() {
		let src = "class Foo:\n    def __secret(self):\n        pass\n";
		let g = extract_default("foo.py", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(m.visibility, b"private".to_vec());
	}

	#[test]
	fn extract_single_underscore_visibility_is_module() {
		let src = "def _internal():\n    pass\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let f = g.defs().find(|d| d.kind == b"function").expect("function def");
		assert_eq!(f.visibility, b"module".to_vec());
	}

	#[test]
	fn extract_import_module_emits_imports_module() {
		let src = "import os\nimport acme.util as u\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let kinds: Vec<&[u8]> = g.refs().map(|r| r.kind.as_slice()).collect();
		assert_eq!(kinds.iter().filter(|k| **k == b"imports_module").count(), 2);
	}

	#[test]
	fn extract_stdlib_import_marks_external() {
		let g = extract_default("m.py", "import json\n", &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module");
		assert_eq!(r.confidence, b"external".to_vec());
	}

	#[test]
	fn extract_project_import_marks_imported() {
		let g = extract_default("m.py", "import acme.util\n", &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_module")
			.expect("imports_module");
		assert_eq!(r.confidence, b"imported".to_vec());
	}

	#[test]
	fn extract_from_import_emits_one_imports_symbol_per_name() {
		let src = "from acme.util import a, b as c\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let names: Vec<&[u8]> = g
			.refs()
			.filter(|r| r.kind == b"imports_symbol")
			.map(|r| r.target.as_view().segments().last().unwrap().name)
			.collect();
		// Both targets are arity-only callable shapes: `a()`, `b()`.
		assert_eq!(names, vec![&b"a()"[..], &b"b()"[..]]);
		// Alias is preserved on the second.
		let aliased = g
			.refs()
			.find(|r| r.kind == b"imports_symbol" && r.alias == b"c")
			.expect("aliased import");
		assert_eq!(aliased.alias, b"c".to_vec());
	}

	#[test]
	fn extract_relative_import_preserves_leading_dots() {
		let src = "from .util import helper\n";
		let g = extract_default("acme/m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_symbol")
			.expect("imports_symbol");
		// The synthetic head segment `external_pkg:.` carries the dot count.
		let segs = r.target.as_view().segments().collect::<Vec<_>>();
		assert_eq!(segs[0].kind, b"external_pkg");
		assert_eq!(segs[0].name, b".");
	}

	#[test]
	fn extract_decorator_emits_annotates() {
		let src = "import functools\n@functools.wraps(fn)\ndef g():\n    pass\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let ann = g
			.refs()
			.find(|r| r.kind == b"annotates")
			.expect("annotates ref");
		// Target last segment is `class:wraps` (by canonicalization rule
		// — decorator name resolved as a class kind for the moniker).
		assert_eq!(
			ann.target.as_view().segments().last().unwrap().name,
			b"wraps"
		);
	}

	#[test]
	fn extract_base_class_emits_extends() {
		let src = "class A:\n    pass\nclass B(A):\n    pass\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let extends_a = g
			.refs()
			.find(|r| r.kind == b"extends")
			.expect("extends ref");
		assert_eq!(extends_a.confidence, b"resolved".to_vec());
		let last = extends_a.target.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"class");
		assert_eq!(last.name, b"A");
	}

	#[test]
	fn extract_method_call_carries_receiver_hint_self() {
		let src = "class Foo:\n    def m(self):\n        self.bar()\n    def bar(self):\n        pass\n";
		let g = extract_default("foo.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"method_call")
			.expect("method_call ref");
		assert_eq!(r.receiver_hint, b"self".to_vec());
	}

	#[test]
	fn extract_call_with_imported_name_marks_imported_confidence() {
		let src = "from acme import helper\ndef f():\n    helper()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"calls"
				&& r.target.as_view().segments().last().unwrap().name == b"helper()")
			.expect("calls helper");
		assert_eq!(r.confidence, b"imported".to_vec());
	}

	#[test]
	fn extract_param_read_marks_confidence_local() {
		let src = "def f(x):\n    return x\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"reads"
				&& r.target.as_view().segments().last().unwrap().name == b"x")
			.expect("reads x");
		assert_eq!(r.confidence, b"local".to_vec());
	}

	#[test]
	fn extract_deep_emits_param_def() {
		let src = "def f(x, y: int):\n    return x\n";
		let g = extract_default("m.py", src, &make_anchor(), true);
		let params: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"param")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert!(params.contains(&&b"x"[..]));
		assert!(params.contains(&&b"y"[..]));
	}

	#[test]
	fn extract_typed_param_emits_uses_type() {
		let src = "def f(x: int):\n    return x\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"uses_type"
				&& r.target.as_view().segments().last().unwrap().name == b"int")
			.expect("uses_type int");
		// stdlib int is not in our type_table → name_match is fine.
		assert!(matches!(
			r.confidence.as_slice(),
			b"name_match" | b"resolved"
		));
	}

	#[test]
	fn extract_subscript_type_descends_into_arguments() {
		let src = "from typing import List\ndef f(xs: List[int]) -> List[int]:\n    return xs\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let kinds: Vec<&[u8]> = g
			.refs()
			.filter(|r| r.kind == b"uses_type")
			.map(|r| r.target.as_view().segments().last().unwrap().name)
			.collect();
		assert!(kinds.contains(&&b"List"[..]));
		assert!(kinds.contains(&&b"int"[..]));
	}
}
