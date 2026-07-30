use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;

use crate::lang::{ExtractionContext, KindSpec, LangExtractor, ParsedDocument};

pub mod build;
mod canonicalize;
mod kinds;
mod sdk_pipeline;

#[derive(Clone, Debug, Default)]
pub struct Presets {}

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_python::LANGUAGE.into();
	parser.set_language(&language).unwrap_or_else(|err| {
		panic!("failed to load tree-sitter Python grammar: {err}");
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
	<Lang as LangExtractor>::extract(uri, source, anchor, deep, presets)
}

pub struct Lang;

const DEF_KINDS: &[&str] = &[
	"class",
	"type",
	"function",
	"method",
	"async_function",
	"path",
];

const DEF_KIND_SPECS: &[KindSpec] = &[
	KindSpec::new("class", Shape::Type, 20, "class"),
	KindSpec::new("type", Shape::Type, 21, "type"),
	KindSpec::new("function", Shape::Callable, 40, "function"),
	KindSpec::new("async_function", Shape::Callable, 41, "async_function"),
	KindSpec::new("method", Shape::Callable, 42, "method"),
	KindSpec::new("path", Shape::Value, 63, "path"),
];

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "python";
	const ALLOWED_KINDS: &'static [&'static str] = DEF_KINDS;
	const KIND_SPECS: &'static [KindSpec] = DEF_KIND_SPECS;
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &["public", "private", "module"];

	fn parse(_uri: &str, source: &str) -> ParsedDocument {
		ParsedDocument::new(parse(source))
	}

	fn file_root(uri: &str, anchor: &Moniker) -> Option<Moniker> {
		Some(canonicalize::compute_module_moniker(anchor, uri))
	}

	fn extract_parsed(
		context: ExtractionContext<'_, Self::Presets>,
		document: &ParsedDocument,
	) -> CodeGraph {
		sdk_pipeline::extract(
			context.uri,
			context.source,
			document,
			context.anchor,
			context.deep,
			context.presets,
		)
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
		let kinds: Vec<&[u8]> = g.refs().map(|r| r.kind.as_ref()).collect();
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
	fn extract_future_annotations_does_not_emit_a_runtime_read() {
		let g = extract_default(
			"m.py",
			"from __future__ import annotations\n",
			&make_anchor(),
			false,
		);
		assert!(!g.refs().any(|reference| {
			reference.kind == b"reads"
				&& reference.target.as_view().segments().last().unwrap().name == b"annotations"
		}));
	}

	#[test]
	fn extract_runtime_module_globals_are_external() {
		let g = extract_default(
			"m.py",
			"if __name__ == '__main__':\n    pass\n",
			&make_anchor(),
			false,
		);
		let reference = g
			.refs()
			.find(|reference| reference.kind == b"reads")
			.expect("reads __name__");
		assert_eq!(reference.confidence, b"external".to_vec());
		assert_eq!(
			reference.target.as_view().segments().next().unwrap().name,
			b"python"
		);
		assert_eq!(
			reference.target.as_view().segments().nth(1).unwrap().name,
			b"runtime"
		);
	}

	#[test]
	fn extract_wildcard_import_preserves_the_star_binding() {
		let g = extract_default(
			"acme/facade.py",
			"from .implementation import *\n",
			&make_anchor(),
			false,
		);
		let reference = g
			.refs()
			.find(|reference| reference.kind == b"imports_module")
			.expect("wildcard module import");

		assert_eq!(reference.alias, b"*".to_vec());
	}

	#[test]
	fn extract_static_all_emits_named_export_bindings() {
		let src = "__all__ = [\n    \"PublicClient\",\n    # Kept public for compatibility.\n    'helper',\n]\nclass PublicClient:\n    pass\ndef helper():\n    pass\n";
		let g = extract_default("acme/implementation.py", src, &make_anchor(), false);
		let exports = g
			.refs()
			.filter(|reference| reference.kind == b"reexports" && !reference.alias.is_empty())
			.map(|reference| reference.alias.clone())
			.collect::<Vec<_>>();

		assert_eq!(exports, vec![b"PublicClient".to_vec(), b"helper".to_vec()]);
	}

	#[test]
	fn extract_all_assignments_preserve_empty_dynamic_and_extend_state() {
		let src = "__all__ = []\n__all__ += ['Later']\n__all__ = build_exports()\n";
		let g = extract_default("acme/facade.py", src, &make_anchor(), false);
		let directives = g
			.refs()
			.filter(|reference| reference.kind == b"reexports" && reference.alias.is_empty())
			.map(|reference| reference.receiver_hint.clone())
			.collect::<Vec<_>>();

		assert_eq!(
			directives,
			vec![
				b"python_all_replace".to_vec(),
				b"python_all_extend".to_vec(),
				b"python_all_dynamic".to_vec(),
			]
		);
	}

	#[test]
	fn extract_conditional_import_marks_runtime_binding() {
		let src = "if enabled:\n    from acme.client import Client\n";
		let g = extract_default("acme/facade.py", src, &make_anchor(), false);
		let reference = g
			.refs()
			.find(|reference| reference.kind == b"imports_symbol")
			.expect("conditional import");

		assert_eq!(
			reference.receiver_hint,
			b"python_conditional_import".to_vec()
		);
	}

	#[test]
	fn extract_calls_through_conditional_imports_as_runtime_bindings() {
		let src = "def build(enabled):\n    if enabled:\n        from acme.client import Client\n    return Client()\n";
		let g = extract_default("acme/facade.py", src, &make_anchor(), false);
		let reference = g
			.refs()
			.find(|reference| reference.kind == b"calls")
			.expect("call through conditional import");

		assert_eq!(
			reference.receiver_hint,
			b"python_conditional_import".to_vec()
		);
	}

	#[test]
	fn extract_function_imports_do_not_leak_into_sibling_scopes() {
		let src = "def configure():\n    from acme.client import Client\n    return Client()\n\ndef build():\n    return Client()\n";
		let g = extract_default("acme/facade.py", src, &make_anchor(), false);
		let calls = g
			.refs()
			.filter(|reference| reference.kind == b"calls")
			.map(|reference| {
				let source = g
					.def_at(reference.source)
					.moniker
					.as_view()
					.segments()
					.last()
					.expect("call source")
					.name
					.to_vec();
				(source, reference.confidence.to_vec())
			})
			.collect::<Vec<_>>();

		assert!(
			calls.contains(&(b"configure()".to_vec(), b"imported".to_vec())),
			"{calls:?}"
		);
		assert!(
			calls.contains(&(b"build()".to_vec(), b"name_match".to_vec())),
			"{calls:?}"
		);
	}

	#[test]
	fn extract_local_import_shadows_conditional_module_binding() {
		let src = "try:\n    from acme.a import Client\nexcept ImportError:\n    from acme.b import Client\n\ndef build():\n    from acme.c import Client\n    return Client()\n";
		let g = extract_default("acme/facade.py", src, &make_anchor(), false);
		let call = g
			.refs()
			.find(|reference| reference.kind == b"calls")
			.expect("locally shadowed call");

		assert_eq!(call.receiver_hint, b"");
		assert_eq!(call.confidence, b"imported");
		assert!(
			call.target
				.as_view()
				.segments()
				.any(|segment| { segment.kind == b"module" && segment.name == b"c" })
		);
	}

	#[test]
	fn extract_conditional_all_as_dynamic() {
		let src = "if enabled:\n    __all__ = ['Client']\nelse:\n    __all__ = ['Fallback']\n";
		let g = extract_default("acme/facade.py", src, &make_anchor(), false);
		let directives = g
			.refs()
			.filter(|reference| reference.kind == b"reexports")
			.map(|reference| (reference.alias.to_vec(), reference.receiver_hint.to_vec()))
			.collect::<Vec<_>>();

		assert_eq!(
			directives,
			vec![
				(Vec::new(), b"python_all_dynamic".to_vec()),
				(Vec::new(), b"python_all_dynamic".to_vec()),
			]
		);
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
	fn extract_async_function_tracks_local_assignments() {
		let src = "async def f():\n    x = 1\n    return x\n";
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
	fn extract_imported_identifier_read_targets_import_moniker() {
		let src = "import asyncio\nasync def f():\n    return asyncio\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"reads" && r.confidence == b"external")
			.expect("external read");
		let segs: Vec<_> = r.target.as_view().segments().collect();
		assert_eq!(segs[0].kind, b"sdk");
		assert_eq!(segs[0].name, b"python");
		assert_eq!(segs[1].name, b"asyncio");
	}

	#[test]
	fn extract_unknown_identifier_read_marks_incomplete_resolution() {
		let src = "def f():\n    return missing_name\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"reads"
					&& r.target.as_view().segments().last().unwrap().name == b"missing_name"
			})
			.expect("reads missing_name");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_same_module_callable_read_keeps_exact_resolution() {
		let src = "def f():\n    return callback\n\ndef callback():\n    pass\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"reads"
					&& r.target.as_view().segments().last().unwrap().name == b"callback()"
			})
			.expect("reads callback");
		assert_eq!(r.confidence, b"resolved".to_vec());
	}

	#[test]
	fn extract_module_callable_read_respects_local_shadowing() {
		let src = "def callback():\n    pass\n\ndef f(callback):\n    return callback\n";
		let g = extract_default("m.py", src, &make_anchor(), true);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"reads"
					&& r.target.as_view().segments().last().unwrap().name == b"callback"
			})
			.expect("reads local callback");
		assert_eq!(r.confidence, b"local".to_vec());
		assert_eq!(r.target.as_view().segments().last().unwrap().kind, b"local");
	}

	#[test]
	fn extract_unknown_receiver_method_call_marks_incomplete_resolution() {
		let src = "def f(value):\n    return value.normalize()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"normalize"
			})
			.expect("method_call normalize");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_self_member_call_resolves_via_typed_constructor_param() {
		let src = "class Store:\n    def reserve(self) -> None:\n        pass\n\nclass Worker:\n    def __init__(self, store: Store) -> None:\n        self._store = store\n\n    def run(self) -> None:\n        self._store.reserve()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"reserve()"
			})
			.expect("method_call Store.reserve");
		assert_eq!(r.confidence, b"resolved".to_vec());
		let parent = r.target.parent().expect("method parent");
		assert_eq!(parent.as_view().segments().last().unwrap().name, b"Store");
	}

	#[test]
	fn extract_self_member_call_uses_constructor_type_independent_of_method_order() {
		let src = "class Store:\n    def reserve(self) -> None:\n        pass\n\nclass Other:\n    def reserve(self) -> None:\n        pass\n\nclass Worker:\n    def before_init(self) -> None:\n        self._store.reserve()\n\n    def __init__(self, store: Store) -> None:\n        self._store = store\n\n    def retarget(self, other: Other) -> None:\n        self._store = other\n\n    def after_retarget(self) -> None:\n        self._store.reserve()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let targets: Vec<Moniker> = g
			.refs()
			.filter(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"reserve()"
			})
			.map(|r| r.target.clone())
			.collect();
		assert_eq!(targets.len(), 2);
		for target in targets {
			let parent = target.parent().expect("method parent");
			assert_eq!(parent.as_view().segments().last().unwrap().name, b"Store");
		}
	}

	#[test]
	fn extract_self_callable_attr_call_targets_attribute_type_alias() {
		let src = "from collections.abc import Callable\nfrom typing import TypeAlias\n\nCallback: TypeAlias = Callable[[], None]\n\nclass Worker:\n    def __init__(self, cb: Callback) -> None:\n        self._cb = cb\n\n    def run(self) -> None:\n        self._cb()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"Callback"
			})
			.expect("calls Callback type alias");
		assert_eq!(r.confidence, b"resolved".to_vec());
		assert!(!g.refs().any(|r| {
			r.kind == b"method_call" && r.target.as_view().segments().last().unwrap().name == b"_cb"
		}));
	}

	#[test]
	fn extract_type_alias_emits_type_def_and_rhs_uses_type() {
		let src = "from typing import TypeAlias\n\nclass User:\n    pass\n\nUserMap: TypeAlias = dict[str, User]\ntype UserResult = User | None\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let type_names: Vec<&[u8]> = g
			.defs()
			.filter(|d| d.kind == b"type")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name)
			.collect();
		assert!(type_names.contains(&&b"UserMap"[..]));
		assert!(type_names.contains(&&b"UserResult"[..]));
		assert!(g.refs().any(|r| {
			r.kind == b"uses_type"
				&& r.target.as_view().segments().last().unwrap().name == b"User"
				&& r.confidence == b"resolved"
		}));
		assert!(!g.refs().any(|r| {
			r.kind == b"uses_type"
				&& matches!(
					r.target.as_view().segments().last().unwrap().name,
					b"dict" | b"str" | b"None"
				)
		}));
	}

	#[test]
	fn extract_local_class_call_emits_instantiates() {
		let src = "class User:\n    pass\n\ndef make() -> User:\n    return User()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates")
			.expect("instantiates User");
		assert_eq!(r.confidence, b"resolved".to_vec());
		assert_eq!(r.target.as_view().segments().last().unwrap().name, b"User");
	}

	#[test]
	fn extract_callable_return_annotation_emits_returns_type() {
		let src = "class User:\n    pass\n\ndef make() -> User:\n    return User()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"returns_type")
			.expect("returns_type User");
		assert_eq!(r.confidence, b"resolved".to_vec());
		assert_eq!(r.target.as_view().segments().last().unwrap().kind, b"class");
		assert_eq!(r.target.as_view().segments().last().unwrap().name, b"User");
	}

	#[test]
	fn extract_local_assignment_uses_annotated_factory_return_type() {
		let src = "class User:\n    def label(self) -> str:\n        return 'user'\n\ndef make() -> User:\n    return User()\n\ndef render():\n    value = make()\n    return value.label()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"label()"
			})
			.expect("method_call User.label");
		assert_eq!(r.confidence, b"resolved".to_vec());
		assert_eq!(
			r.target
				.parent()
				.expect("method parent")
				.as_view()
				.segments()
				.last()
				.unwrap()
				.name,
			b"User"
		);
	}

	#[test]
	fn extract_shadowed_factory_does_not_leak_module_return_type() {
		let src = "class User:\n    def label(self) -> str:\n        return 'user'\n\ndef make() -> User:\n    return User()\n\ndef render(make):\n    value = make()\n    return value.label()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"label"
			})
			.expect("method_call label");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_later_assignment_still_shadows_module_factory() {
		let src = "class User:\n    def label(self) -> str:\n        return 'user'\n\ndef make() -> User:\n    return User()\n\ndef render():\n    value = make()\n    make = lambda: None\n    return value.label()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"label"
			})
			.expect("method_call label");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_factory_return_type_uses_defining_scope() {
		let src = "class Product:\n    def label(self) -> str:\n        return 'product'\n\ndef make() -> Product:\n    return Product()\n\nclass View:\n    class Product:\n        def label(self) -> str:\n            return 'shadow'\n\n    def render(self):\n        value = make()\n        return value.label()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& g.defs().nth(r.source).is_some_and(|source| {
						source
							.moniker
							.as_view()
							.segments()
							.last()
							.is_some_and(|segment| segment.name == b"render()")
					}) && r.target.as_view().segments().last().unwrap().name == b"label()"
			})
			.expect("method_call Product.label");
		let owner = r.target.parent().expect("method owner");
		assert_eq!(owner.as_view().segments().last().unwrap().name, b"Product");
		assert_eq!(owner.parent(), Some(g.root().clone()));
	}

	#[test]
	fn extract_bare_call_in_method_does_not_bind_sibling_method() {
		let src = "class Product:\n    def label(self):\n        pass\n\nclass Factory:\n    def make(self) -> Product:\n        return Product()\n\n    def render(self):\n        value = make()\n        return value.label()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"label"
			})
			.expect("method_call label");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_builtin_annotated_receiver_method_is_external() {
		let src = "def normalize(value: str) -> str:\n    return value.strip()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"strip"
			})
			.expect("method_call str.strip");
		assert_eq!(r.confidence, b"external".to_vec());
		let segments = r.target.as_view().segments().collect::<Vec<_>>();
		assert_eq!(segments[0].kind, b"sdk");
		assert_eq!(segments[0].name, b"python");
		assert_eq!(segments[1].name, b"builtins");
		assert_eq!(segments[2].name, b"str");
	}

	#[test]
	fn extract_builtin_generic_receiver_uses_container_type() {
		let src = "def add(values: list[str]):\n    values.append('x')\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|reference| reference.kind == b"method_call")
			.expect("method_call list.append");
		assert_eq!(r.confidence, b"external".to_vec());
		assert_eq!(r.target.as_view().segments().nth(2).unwrap().name, b"list");
	}

	#[test]
	fn extract_variadic_parameters_use_runtime_container_types() {
		let src = "def collect(*items: str, **options: int):\n    items.count('x')\n    options.get('limit')\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let targets = g
			.refs()
			.filter(|reference| reference.kind == b"method_call")
			.map(|reference| {
				reference
					.target
					.as_view()
					.segments()
					.nth(2)
					.unwrap()
					.name
					.to_vec()
			})
			.collect::<Vec<_>>();
		assert!(targets.contains(&b"tuple".to_vec()), "{targets:?}");
		assert!(targets.contains(&b"dict".to_vec()), "{targets:?}");
	}

	#[test]
	fn extract_with_alias_preserves_constructed_receiver_type() {
		let src = "class Session:\n    def __enter__(self) -> Session:\n        return self\n\n    def __exit__(self, exc_type, exc, tb):\n        pass\n\n    def close(self):\n        pass\n\ndef run():\n    with Session() as session:\n        session.close()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|reference| reference.kind == b"method_call")
			.expect("method_call Session.close");
		assert_eq!(r.confidence, b"resolved".to_vec());
		assert_eq!(
			r.target
				.parent()
				.unwrap()
				.as_view()
				.segments()
				.last()
				.unwrap()
				.name,
			b"Session"
		);
	}

	#[test]
	fn extract_annotated_iterable_types_the_loop_binding() {
		let src = "class Item:\n    def label(self):\n        pass\n\ndef render(items: list[Item]):\n    for item in items:\n        item.label()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|reference| reference.kind == b"method_call")
			.expect("method_call Item.label");
		assert_eq!(r.confidence, b"resolved".to_vec());
		assert_eq!(
			r.target
				.parent()
				.unwrap()
				.as_view()
				.segments()
				.last()
				.unwrap()
				.name,
			b"Item"
		);
	}

	#[test]
	fn extract_heterogeneous_tuple_annotation_keeps_all_loop_element_types() {
		let src = "class Alpha:\n    pass\n\nclass Beta:\n    pass\n\ndef render(values: tuple[Alpha, Beta]):\n    for value in values:\n        pass\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let mut targets = g
			.refs()
			.filter(|reference| reference.kind == b"typed_as" && reference.alias == b"value")
			.map(|reference| {
				reference
					.target
					.as_view()
					.segments()
					.last()
					.unwrap()
					.name
					.to_vec()
			})
			.collect::<Vec<_>>();
		targets.sort();
		assert_eq!(targets, vec![b"Alpha".to_vec(), b"Beta".to_vec()]);
	}

	#[test]
	fn extract_except_alias_uses_the_exception_type() {
		let src = "class Problem(Exception):\n    def explain(self):\n        pass\n\ndef run():\n    try:\n        pass\n    except Problem as error:\n        error.explain()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|reference| reference.kind == b"method_call")
			.expect("method_call Problem.explain");
		assert_eq!(r.confidence, b"resolved".to_vec());
		assert_eq!(
			r.target
				.parent()
				.unwrap()
				.as_view()
				.segments()
				.last()
				.unwrap()
				.name,
			b"Problem"
		);
	}

	#[test]
	fn extract_exception_tuple_keeps_all_alias_types() {
		let src = "class AlphaError(Exception):\n    pass\n\nclass BetaError(Exception):\n    pass\n\ndef run():\n    try:\n        pass\n    except (AlphaError, BetaError) as error:\n        pass\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let mut targets = g
			.refs()
			.filter(|reference| reference.kind == b"typed_as" && reference.alias == b"error")
			.map(|reference| {
				reference
					.target
					.as_view()
					.segments()
					.last()
					.unwrap()
					.name
					.to_vec()
			})
			.collect::<Vec<_>>();
		targets.sort();
		assert_eq!(targets, vec![b"AlphaError".to_vec(), b"BetaError".to_vec()]);
	}

	#[test]
	fn extract_distinct_builtin_union_does_not_invent_receiver_type() {
		let src = "def normalize(value: str | bytes):\n    return value.strip()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"strip"
			})
			.expect("method_call strip");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_workspace_union_parameter_emits_each_local_type_fact() {
		let src = "class Alpha:\n    pass\n\nclass Beta:\n    pass\n\ndef render(value: Alpha | Beta):\n    return value\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let mut targets = g
			.refs()
			.filter(|reference| reference.kind == b"typed_as" && reference.alias == b"value")
			.map(|reference| {
				reference
					.target
					.as_view()
					.segments()
					.last()
					.unwrap()
					.name
					.to_vec()
			})
			.collect::<Vec<_>>();
		targets.sort();
		assert_eq!(targets, vec![b"Alpha".to_vec(), b"Beta".to_vec()]);
	}

	#[test]
	fn extract_optional_builtin_does_not_invent_receiver_type() {
		let src = "from typing import Optional\n\ndef normalize(value: Optional[str]):\n    return value.strip()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"strip"
			})
			.expect("method_call strip");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_workspace_type_shadows_builtin_name() {
		let src = "class str:\n    def custom(self):\n        pass\n\ndef use(value: str):\n    value.custom()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"custom()"
			})
			.expect("method_call workspace str.custom");
		assert_eq!(r.confidence, b"resolved".to_vec());
		assert_eq!(r.target.parent().unwrap().parent(), Some(g.root().clone()));
	}

	#[test]
	fn extract_literal_assignment_infers_builtin_receiver_type() {
		let src = "def normalize():\n    value = ' user '\n    return value.strip()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"strip"
			})
			.expect("method_call str.strip");
		assert_eq!(r.confidence, b"external".to_vec());
		assert_eq!(r.target.as_view().segments().nth(2).unwrap().name, b"str");
	}

	#[test]
	fn extract_raw_bytes_literal_infers_bytes_receiver_type() {
		let src = "def decode():\n    value = rb'user'\n    return value.decode()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"decode"
			})
			.expect("method_call bytes.decode");
		assert_eq!(r.confidence, b"external".to_vec());
		assert_eq!(r.target.as_view().segments().nth(2).unwrap().name, b"bytes");
	}

	#[test]
	fn extract_builtin_return_annotation_emits_external_returns_type() {
		let src = "def label() -> str:\n    return 'user'\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"returns_type")
			.expect("returns_type str");
		assert_eq!(r.confidence, b"external".to_vec());
		assert_eq!(r.target.as_view().segments().nth(2).unwrap().name, b"str");
	}

	#[test]
	fn extract_async_return_type_requires_await_for_local_inference() {
		let src = "class User:\n    def label(self) -> str:\n        return 'user'\n\nasync def make() -> User:\n    return User()\n\nasync def direct():\n    value = make()\n    return value.label()\n\nasync def awaited():\n    value = await make()\n    return value.label()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		assert!(!g.refs().any(|r| r.kind == b"returns_type" && {
			g.defs().nth(r.source).is_some_and(|source| {
				source
					.moniker
					.as_view()
					.segments()
					.last()
					.is_some_and(|segment| segment.name == b"make()")
			})
		}));
		let method_calls = g
			.refs()
			.filter(|r| r.kind == b"method_call")
			.map(|r| {
				let source = g.defs().nth(r.source).unwrap();
				(
					source
						.moniker
						.as_view()
						.segments()
						.last()
						.unwrap()
						.name
						.to_vec(),
					r.confidence.to_vec(),
				)
			})
			.collect::<Vec<_>>();
		assert!(method_calls.contains(&(b"direct()".to_vec(), b"unresolved".to_vec())));
		assert!(method_calls.contains(&(b"awaited()".to_vec(), b"resolved".to_vec())));
	}

	#[test]
	fn extract_async_method_does_not_emit_direct_return_type() {
		let src = "class User:\n    pass\n\nclass Factory:\n    async def make(self) -> User:\n        return User()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		assert!(!g.refs().any(|r| r.kind == b"returns_type"));
	}

	#[test]
	fn extract_conflicting_factory_returns_do_not_type_local_assignment() {
		let src = "class First:\n    def marker(self):\n        pass\n\nclass Second:\n    def marker(self):\n        pass\n\ndef make() -> First:\n    return First()\n\ndef make() -> Second:\n    return Second()\n\ndef render():\n    value = make()\n    return value.marker()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"marker"
			})
			.expect("method_call marker");
		assert_eq!(r.confidence, b"unresolved".to_vec());
		assert!(!g.refs().any(|r| r.kind == b"returns_type"));
	}

	#[test]
	fn extract_local_import_shadows_module_factory_return() {
		let src = "class Local:\n    def marker(self):\n        pass\n\ndef make() -> Local:\n    return Local()\n\ndef render():\n    from other import make\n    value = make()\n    return value.marker()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"marker"
			})
			.expect("method_call marker");
		assert_ne!(
			r.target
				.parent()
				.unwrap()
				.as_view()
				.segments()
				.last()
				.unwrap()
				.name,
			b"Local"
		);
	}

	#[test]
	fn extract_aliased_optional_does_not_become_receiver_type() {
		let src = "from typing import Optional as Maybe\n\ndef normalize(value: Maybe[str]):\n    return value.strip()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"strip"
			})
			.expect("method_call strip");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_typing_any_remains_dynamic() {
		let src = "from typing import Any\n\ndef make() -> Any:\n    raise RuntimeError\n\ndef use(value: Any):\n    return value.dynamic()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		assert!(!g.refs().any(|r| r.kind == b"returns_type"));
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"dynamic"
			})
			.expect("method_call dynamic");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_conflicting_local_assignment_types_remain_ambiguous() {
		let src = "def mutate(flag):\n    if flag:\n        value = []\n    else:\n        value = {}\n    value.append(1)\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"append"
			})
			.expect("method_call append");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_unknown_reassignment_invalidates_known_local_type() {
		let src = "def mutate():\n    value = []\n    value = unknown()\n    value.append(1)\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"append"
			})
			.expect("method_call append");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_conflicting_instance_attribute_types_remain_ambiguous() {
		let src = "class Holder:\n    def mutate(self, flag):\n        if flag:\n            self.value = []\n        else:\n            self.value = {}\n        self.value.append(1)\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"append"
			})
			.expect("method_call append");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_qualified_typing_any_does_not_bind_workspace_homonym() {
		let src = "import typing\n\nclass Any:\n    def dynamic(self):\n        pass\n\ndef use(value: typing.Any):\n    value.dynamic()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"dynamic"
			})
			.expect("method_call dynamic");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_match_capture_shadows_module_factory() {
		let src = "class Local:\n    def marker(self):\n        pass\n\ndef make() -> Local:\n    return Local()\n\ndef render(subject):\n    value = make()\n    match subject:\n        case make:\n            pass\n    return value.marker()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"marker"
			})
			.expect("method_call marker");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_comprehension_walrus_shadows_module_factory() {
		let src = "class Local:\n    def marker(self):\n        pass\n\ndef make() -> Local:\n    return Local()\n\ndef render(items):\n    value = make()\n    selected = [item for item in items if (make := item)]\n    return value.marker()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"marker"
			})
			.expect("method_call marker");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_inner_param_shadows_outer_inferred_type() {
		let src =
			"def outer():\n    value = []\n\n    def inner(value):\n        value.append(1)\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"method_call"
					&& r.target.as_view().segments().last().unwrap().name == b"append"
			})
			.expect("method_call append");
		assert_eq!(r.confidence, b"unresolved".to_vec());
	}

	#[test]
	fn extract_open_union_return_keeps_known_type_with_open_marker() {
		let src = "class User:\n    pass\n\ndef maybe_make() -> User | None:\n    return None\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let fact = g
			.refs()
			.find(|reference| reference.kind == b"returns_type")
			.expect("known User return candidate");
		assert_eq!(
			fact.target.as_view().segments().last().unwrap().name,
			b"User"
		);
		assert_eq!(fact.receiver_hint, b"python_open_type_set".to_vec());
	}

	#[test]
	fn extract_closed_union_return_emits_each_type_fact() {
		let src = "class Alpha:\n    pass\n\nclass Beta:\n    pass\n\ndef make() -> Alpha | Beta:\n    return Alpha()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let mut targets = g
			.refs()
			.filter(|reference| reference.kind == b"returns_type")
			.map(|reference| {
				reference
					.target
					.as_view()
					.segments()
					.last()
					.unwrap()
					.name
					.to_vec()
			})
			.collect::<Vec<_>>();
		targets.sort();
		assert_eq!(targets, vec![b"Alpha".to_vec(), b"Beta".to_vec()]);
	}

	#[test]
	fn extract_local_class_call_prefers_function_scoped_type() {
		let src = "class Local:\n    pass\n\ndef make():\n    class Local:\n        pass\n    return Local()\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| r.kind == b"instantiates")
			.expect("instantiates local Local");
		let target_last = r.target.as_view().segments().last().unwrap();
		assert_eq!(target_last.kind, b"class");
		assert_eq!(target_last.name, b"Local");
		let parent = r.target.parent().expect("class parent");
		let parent_last = parent.as_view().segments().last().unwrap();
		assert_eq!(parent_last.kind, b"function");
		assert_eq!(parent_last.name, b"make()");
	}

	#[test]
	fn extract_keyword_argument_names_are_not_reads() {
		let src = "def save(value):\n    return dict(id=value)\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		assert!(!g.refs().any(|r| {
			r.kind == b"reads" && r.target.as_view().segments().last().unwrap().name == b"id"
		}));
	}

	#[test]
	fn extract_attribute_tail_is_not_a_bare_read() {
		let src = "def f(payment):\n    return payment.id\n";
		let g = extract_default("m.py", src, &make_anchor(), false);
		assert!(!g.refs().any(|r| {
			r.kind == b"reads" && r.target.as_view().segments().last().unwrap().name == b"id"
		}));
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
