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
	let (def_cap, ref_cap) = CodeGraph::capacity_for_source(source.len());
	let mut graph = CodeGraph::with_capacity(module.clone(), kinds::MODULE, def_cap, ref_cap);
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
		false,
		&mut callable_table,
	);
	let strat = Strategy {
		module: module.clone(),
		source_bytes: source.as_bytes(),
		deep,
		imports: RefCell::new(HashMap::<Vec<u8>, &'static [u8]>::new()),
		import_targets: RefCell::new(HashMap::<Vec<u8>, _>::new()),
		local_scope: RefCell::new(Vec::new()),
		type_table,
		callable_table,
	};
	let walker = CanonicalWalker::new(&strat, source.as_bytes());
	walker.walk(tree.root_node(), &module, &mut graph);
	if let Some(docstring) = strategy::first_docstring(tree.root_node()) {
		strategy::emit_docstring_def(docstring, &module, &mut graph);
	}
	graph
}

pub struct Lang;

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "python";
	const ALLOWED_KINDS: &'static [&'static str] =
		&["class", "function", "method", "async_function"];
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
	fn parse_empty_returns_module() {
		let tree = parse("");
		assert_eq!(tree.root_node().kind(), "module");
	}

	#[test]
	fn extract_module_uses_path_segments() {
		let g = extract_default("acme/util/text.py", "", &make_anchor(), false);
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"python")
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
			.segment(b"lang", b"python")
			.segment(b"module", b"foo")
			.build();
		assert_eq!(g.root(), &expected);
	}

	#[test]
	fn extract_function_with_typed_params_emits_full_signature() {
		let src = "def make(x: int, y: str) -> int:\n    return x\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let f = g
			.defs()
			.find(|d| d.kind == b"function")
			.expect("function def");
		let last = f.moniker.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"function");
		assert_eq!(last.name, b"make(x:int,y:str)");
		assert_eq!(f.signature, b"x:int,y:str".to_vec());
	}

	#[test]
	fn extract_function_with_untyped_params_uses_name_only_slots() {
		let src = "def f(a, b=1):\n    return a\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let f = g
			.defs()
			.find(|d| d.kind == b"function")
			.expect("function def");
		let last = f.moniker.as_view().segments().last().unwrap();
		assert_eq!(last.name, b"f(a,b)");
		assert_eq!(f.signature, b"a,b".to_vec());
	}

	#[test]
	fn extract_classmethod_excludes_cls_from_signature() {
		let src = "class Foo:\n    @classmethod\n    def make(cls, x: int) -> 'Foo':\n        return cls()\n";
		let g = extract_default("foo.py", src, &make_anchor(), false);
		let m = g.defs().find(|d| d.kind == b"method").expect("method def");
		assert_eq!(
			m.moniker.as_view().segments().last().unwrap().name,
			b"make(x:int)"
		);
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
		let f = g
			.defs()
			.find(|d| d.kind == b"function")
			.expect("function def");
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
		assert_eq!(names, vec![&b"a"[..], &b"b"[..]]);
		let segs: Vec<_> = g
			.refs()
			.find(|r| r.kind == b"imports_symbol")
			.unwrap()
			.target
			.as_view()
			.segments()
			.collect();
		let kinds: Vec<&[u8]> = segs.iter().map(|s| s.kind).collect();
		assert_eq!(
			kinds,
			vec![&b"lang"[..], &b"package"[..], &b"module"[..], &b"path"[..]]
		);
		let aliased = g
			.refs()
			.find(|r| r.kind == b"imports_symbol" && r.alias == b"c")
			.expect("aliased import");
		assert_eq!(aliased.alias, b"c".to_vec());
	}

	#[test]
	fn extract_relative_import_resolves_against_importer() {
		let src = "from .util import helper\n";
		let g = extract_default("acme/m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_symbol")
			.expect("imports_symbol");
		let segs: Vec<_> = r.target.as_view().segments().collect();
		let kinds: Vec<&[u8]> = segs.iter().map(|s| s.kind).collect();
		let names: Vec<&[u8]> = segs.iter().map(|s| s.name).collect();
		assert_eq!(
			kinds,
			vec![&b"lang"[..], &b"package"[..], &b"module"[..], &b"path"[..]]
		);
		assert_eq!(
			names,
			vec![&b"python"[..], &b"acme"[..], &b"util"[..], &b"helper"[..]]
		);
	}

	#[test]
	fn extract_relative_import_underflow_falls_back_to_external_pkg() {
		let src = "from ...foo import bar\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"imports_symbol")
			.expect("imports_symbol");
		let segs: Vec<_> = r.target.as_view().segments().collect();
		assert_eq!(segs[0].kind, b"external_pkg");
		assert_eq!(segs[0].name, b"...");
	}

	#[test]
	fn extract_decorator_emits_annotates() {
		let src = "import functools\n@functools.wraps(fn)\ndef g():\n    pass\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let ann = g
			.refs()
			.find(|r| r.kind == b"annotates")
			.expect("annotates ref");
		assert_eq!(
			ann.target.as_view().segments().last().unwrap().name,
			b"wraps"
		);
	}

	#[test]
	fn extract_param_read_marks_confidence_local() {
		let src = "def f(x):\n    return x\n";
		let g = extract_default("m.py", src, &make_anchor(), true);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"reads" && r.target.as_view().segments().last().unwrap().name == b"x"
			})
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
	fn extract_function_docstring_emits_comment_def_parented_on_function() {
		let src = "def f():\n    \"\"\"docstring\"\"\"\n    return 0\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let fn_moniker = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"python")
			.segment(b"module", b"m")
			.segment(b"function", b"f()")
			.build();
		let docstring_count = g
			.defs()
			.filter(|d| d.kind == b"comment")
			.filter(|d| {
				d.parent
					.and_then(|i| g.defs().nth(i))
					.is_some_and(|p| p.moniker == fn_moniker)
			})
			.count();
		assert_eq!(
			docstring_count,
			1,
			"function docstring must emit one comment def parented on the function. defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_class_and_module_docstring_each_emit_one_comment() {
		let src = "\"\"\"module doc\"\"\"\nclass A:\n    \"\"\"class doc\"\"\"\n    pass\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		assert_eq!(
			g.defs().filter(|d| d.kind == b"comment").count(),
			2,
			"module-level and class docstrings should each yield one comment def. defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_non_docstring_string_at_start_is_not_a_comment() {
		let src = "x = \"hello\"\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		assert_eq!(
			g.defs().filter(|d| d.kind == b"comment").count(),
			0,
			"string literals that aren't bare expression-statement-strings must NOT be treated as docstrings"
		);
	}
}
