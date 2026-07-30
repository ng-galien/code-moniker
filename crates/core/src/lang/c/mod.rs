use std::collections::BTreeSet;
use std::sync::Arc;

use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;

use crate::lang::{ExtractionContext, KindSpec, LangExtractor, ParsedDocument};

mod kinds;
mod sdk_pipeline;

#[derive(Clone, Debug, Default)]
pub struct Presets {
	/// Project-relative compiler include roots, in search order.
	pub include_paths: Vec<String>,
	/// Project-relative paths known to exist in the indexed source root.
	pub workspace_files: Arc<BTreeSet<String>>,
	/// Package owning unresolved quoted headers declared by the build system.
	pub external_include_package: Option<String>,
}

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_c::LANGUAGE.into();
	parser.set_language(&language).unwrap_or_else(|err| {
		panic!("failed to load tree-sitter C grammar: {err}");
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
	"struct",
	"enum",
	"type",
	"func",
	"macro",
	"field",
	"enum_constant",
	"var",
	"const",
];

const DEF_KIND_SPECS: &[KindSpec] = &[
	KindSpec::new("struct", Shape::Type, 20, "struct"),
	KindSpec::new("enum", Shape::Type, 21, "enum"),
	KindSpec::new("type", Shape::Type, 22, "typedef"),
	KindSpec::new("enum_constant", Shape::Value, 30, "enum constant"),
	KindSpec::new("field", Shape::Value, 31, "field"),
	KindSpec::new("func", Shape::Callable, 40, "function"),
	KindSpec::new("macro", Shape::Callable, 41, "macro"),
	KindSpec::new("const", Shape::Value, 60, "define"),
	KindSpec::new("var", Shape::Value, 61, "variable"),
];

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "c";
	const ALLOWED_KINDS: &'static [&'static str] = DEF_KINDS;
	const KIND_SPECS: &'static [KindSpec] = DEF_KIND_SPECS;
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &["public", "module"];

	fn parse(_uri: &str, source: &str) -> ParsedDocument {
		ParsedDocument::new(parse(source))
	}

	fn file_root(uri: &str, anchor: &Moniker) -> Option<Moniker> {
		Some(sdk_pipeline::compute_module_moniker(anchor, uri))
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
	fn parse_empty_returns_translation_unit() {
		let tree = parse("");
		assert_eq!(tree.root_node().kind(), "translation_unit");
	}

	#[test]
	fn extract_module_strips_c_extension_and_keeps_h() {
		let g = extract_default("src/util/text.c", "int x;\n", &make_anchor(), false);
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"dir", b"src")
			.segment(b"dir", b"util")
			.segment(b"module", b"text")
			.build();
		assert_eq!(g.root(), &expected);

		let h = extract_default("src/util/text.h", "int y;\n", &make_anchor(), false);
		let expected_h = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"dir", b"src")
			.segment(b"dir", b"util")
			.segment(b"module", b"text.h")
			.build();
		assert_eq!(h.root(), &expected_h);
	}

	#[test]
	fn extract_function_definition_with_pointer_params() {
		let src = "int run(char *name, int n) { return n; }\n";
		let g = extract_default("main.c", src, &make_anchor(), false);
		let run = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"main")
			.segment(b"func", b"run(name:char*,n:int)")
			.build();
		assert!(
			g.contains(&run),
			"function def expected; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_static_function_is_module_visible() {
		let src = "static void helper(void) {}\n";
		let g = extract_default("main.c", src, &make_anchor(), false);
		let def = g.defs().find(|d| d.kind == b"func").expect("func def");
		assert_eq!(def.visibility, b"module".to_vec());
		let name = def
			.moniker
			.as_view()
			.segments()
			.last()
			.unwrap()
			.name
			.to_vec();
		assert_eq!(name, b"helper()".to_vec(), "(void) collapses to zero slots");
	}

	#[test]
	fn extract_struct_with_fields_and_typedef() {
		let src = "typedef struct obj { int refcount; void *ptr; } obj;\n";
		let g = extract_default("obj.c", src, &make_anchor(), false);
		let strukt = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"obj")
			.segment(b"struct", b"obj")
			.build();
		let field = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"obj")
			.segment(b"struct", b"obj")
			.segment(b"field", b"refcount")
			.build();
		assert!(g.contains(&strukt));
		assert!(g.contains(&field));
	}

	#[test]
	fn extract_anonymous_typedef_struct_owns_fields() {
		let src = "typedef struct { int len; char buf[8]; } sds;\n";
		let g = extract_default("sds.c", src, &make_anchor(), false);
		let ty = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"sds")
			.segment(b"type", b"sds")
			.build();
		let field = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"sds")
			.segment(b"type", b"sds")
			.segment(b"field", b"len")
			.build();
		assert!(g.contains(&ty));
		assert!(g.contains(&field));
	}

	#[test]
	fn extract_enum_constants_are_defs() {
		let src = "enum color { RED, GREEN = 2 };\n";
		let g = extract_default("color.c", src, &make_anchor(), false);
		let red = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"color")
			.segment(b"enum", b"color")
			.segment(b"enum_constant", b"RED")
			.build();
		assert!(g.contains(&red));
	}

	#[test]
	fn extract_macros_as_defs() {
		let src = "#define MAX_LEN 128\n#define MIN(a, b) ((a) < (b) ? (a) : (b))\n";
		let g = extract_default("m.c", src, &make_anchor(), false);
		let object = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"m")
			.segment(b"const", b"MAX_LEN")
			.build();
		let function_like = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"m")
			.segment(b"macro", b"MIN(a,b)")
			.build();
		assert!(g.contains(&object));
		assert!(g.contains(&function_like));
	}

	#[test]
	fn extract_header_guard_is_not_a_definition() {
		let src = "#ifndef UTIL_H\n#define UTIL_H\n#define MAX_LEN 128\n#endif\n";
		let g = extract_default("util.h", src, &make_anchor(), false);
		let names = g
			.defs()
			.filter_map(|definition| {
				definition
					.moniker
					.as_view()
					.segments()
					.last()
					.map(|segment| segment.name.to_vec())
			})
			.collect::<Vec<_>>();
		assert!(!names.iter().any(|name| name == b"UTIL_H"));
		assert!(names.iter().any(|name| name == b"MAX_LEN"));
	}

	#[test]
	fn extract_pointer_fields_and_vars_keep_pointer_signature() {
		let src = "struct item { char *name; }; extern struct item *current;";
		let g = extract_default("types.c", src, &make_anchor(), false);
		let field = g
			.defs()
			.find(|definition| definition.kind == b"field")
			.expect("field definition");
		let variable = g
			.defs()
			.find(|definition| definition.kind == b"var")
			.expect("variable definition");
		assert_eq!(field.signature, b"char*");
		assert_eq!(variable.signature, b"structitem*");
	}

	#[test]
	fn extract_field_before_trailing_attribute_macro() {
		let source = "typedef struct Table { int *value field_attr(ignore); int count field_attr(ignore); } Table;\n";
		let graph = extract_default("table.h", source, &make_anchor(), false);

		assert!(graph.defs().any(|definition| {
			definition.kind == b"field"
				&& definition
					.moniker
					.as_view()
					.segments()
					.last()
					.is_some_and(|segment| segment.name == b"value")
		}));
		assert!(graph.defs().any(|definition| {
			definition.kind == b"field"
				&& definition
					.moniker
					.as_view()
					.segments()
					.last()
					.is_some_and(|segment| segment.name == b"count")
		}));
	}

	#[test]
	fn extract_system_include_is_external_and_local_include_internal() {
		let src = "#include <stdio.h>\n#include <project/api.h>\n#include \"vendor/util.h\"\n";
		let presets = Presets {
			include_paths: vec![String::new()],
			workspace_files: Arc::new(BTreeSet::from([
				"main.c".to_string(),
				"project/api.h".to_string(),
				"vendor/util.h".to_string(),
			])),
			..Presets::default()
		};
		let g = extract("main.c", src, &make_anchor(), false, &presets);
		assert_conformance::<super::Lang>(&g, &make_anchor());
		let external = g
			.refs()
			.find(|r| r.kind == b"imports_module" && r.confidence == b"external".to_vec())
			.expect("system include ref");
		assert!(
			external
				.target
				.as_view()
				.segments()
				.any(|s| s.kind == b"sdk" && s.name == b"c"),
		);
		assert!(
			external
				.target
				.as_view()
				.segments()
				.any(|s| s.kind == b"path" && s.name == b"stdio")
		);
		let internal = g
			.refs()
			.find(|r| r.kind == b"imports_module" && r.confidence == b"imported".to_vec())
			.expect("local include ref");
		assert!(
			internal
				.target
				.as_view()
				.segments()
				.any(|s| s.kind == b"module" && s.name == b"api.h"),
		);
		let quoted = g
			.refs()
			.find(|r| {
				r.kind == b"imports_module"
					&& r.target
						.as_view()
						.segments()
						.any(|s| s.kind == b"module" && s.name == b"util.h")
			})
			.expect("quoted include ref");
		assert!(
			quoted
				.target
				.as_view()
				.segments()
				.any(|segment| { segment.kind == b"dir" && segment.name == b"vendor" })
		);
	}

	#[test]
	fn extract_missing_angle_include_is_external_dependency() {
		let graph = extract_default(
			"main.c",
			"#include <protobuf-c/protobuf-c.h>\n",
			&make_anchor(),
			false,
		);
		let include = graph
			.refs()
			.find(|reference| reference.kind == b"imports_module")
			.expect("include reference");

		assert_eq!(include.confidence, b"external");
		assert!(
			include.target.as_view().segments().any(|segment| {
				segment.kind == b"external_pkg" && segment.name == b"protobuf-c"
			})
		);
	}

	#[test]
	fn pgxs_only_claims_known_postgresql_quoted_headers() {
		let presets = Presets {
			external_include_package: Some("postgresql".to_string()),
			..Presets::default()
		};
		let graph = extract(
			"extension.c",
			"#include \"postgres.h\"\n#include \"local_generated.h\"\n",
			&make_anchor(),
			false,
			&presets,
		);
		let includes = graph.refs().collect::<Vec<_>>();

		assert!(includes.iter().any(|reference| {
			reference.receiver_hint == b"c_build_dependency"
				&& reference
					.target
					.as_view()
					.segments()
					.any(|segment| segment.name == b"postgres")
		}));
		assert!(includes.iter().any(|reference| {
			reference.receiver_hint.is_empty()
				&& reference
					.target
					.as_view()
					.segments()
					.any(|segment| segment.name == b"local_generated.h")
		}));
	}

	#[test]
	fn extract_quoted_include_cannot_escape_workspace_root() {
		let graph = extract_default(
			"src/main.c",
			"#include \"../../secret.h\"\n",
			&make_anchor(),
			false,
		);
		let include = graph
			.refs()
			.find(|reference| reference.kind == b"imports_module")
			.expect("include reference");

		assert_eq!(include.confidence, b"external");
		assert!(
			include.target.as_view().segments().any(|segment| {
				segment.kind == b"external_pkg" && segment.name == b"filesystem"
			})
		);
	}

	#[test]
	fn extract_quoted_include_uses_configured_search_root_when_not_source_relative() {
		let src = "#include \"protobuf/model.h\"\n";
		let presets = Presets {
			include_paths: vec![String::new()],
			workspace_files: Arc::new(BTreeSet::from([
				"examples/main.c".to_string(),
				"protobuf/model.h".to_string(),
			])),
			..Presets::default()
		};
		let graph = extract("examples/main.c", src, &make_anchor(), false, &presets);
		let include = graph
			.refs()
			.find(|reference| reference.kind == b"imports_module")
			.expect("include reference");
		let segments = include.target.as_view().segments().collect::<Vec<_>>();

		assert!(
			segments
				.iter()
				.any(|segment| segment.kind == b"dir" && segment.name == b"protobuf")
		);
		assert!(
			!segments
				.iter()
				.any(|segment| segment.kind == b"dir" && segment.name == b"examples")
		);
	}

	#[test]
	fn extract_function_definition_replaces_forward_declaration_slice() {
		let src = "static int work(int value);\nstatic int work(int value) { return value; }\n";
		let graph = extract_default("main.c", src, &make_anchor(), false);
		let work = graph
			.defs()
			.find(|definition| definition.kind == b"func")
			.expect("work definition");
		assert_eq!(
			work.position.expect("definition position").0,
			src.rfind("static int work").unwrap() as u32,
		);
	}

	#[test]
	fn extract_same_file_call_resolves_and_libc_call_is_external() {
		let src = "#include <string.h>\nstatic int helper(int a) { return a; }\nint run(void) { helper(1); return strlen(\"x\");}\n";
		let g = extract_default("main.c", src, &make_anchor(), false);
		let resolved = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"helper(a:int)"
			})
			.expect("resolved same-file call");
		assert_eq!(resolved.confidence, b"resolved".to_vec());
		let libc = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"strlen"
			})
			.expect("libc call");
		assert_eq!(libc.confidence, b"external".to_vec());
		assert!(
			libc.target
				.as_view()
				.segments()
				.any(|s| s.kind == b"sdk" && s.name == b"c"),
		);
		assert!(
			libc.target
				.as_view()
				.segments()
				.any(|s| s.kind == b"path" && s.name == b"libc")
		);
	}

	#[test]
	fn extract_unresolved_bare_call_stays_name_match_with_hints() {
		let src = "int run(void) { return listLength(0); }\n";
		let g = extract_default("main.c", src, &make_anchor(), false);
		let r = g
			.refs()
			.find(|r| {
				r.kind == b"calls"
					&& r.target.as_view().segments().last().unwrap().name == b"listLength"
			})
			.expect("bare unresolved call");
		assert_eq!(r.confidence, b"name_match".to_vec());
		assert_eq!(r.call_name, b"listLength".to_vec());
		assert_eq!(r.call_arity, Some(1));
	}

	#[test]
	fn extract_typed_receiver_field_call_targets_the_field() {
		let src =
			"typedef struct vt { void (*free)(void *p); } vt;\nvoid run(vt *v) { v->free(0); }\n";
		let g = extract_default("vt.c", src, &make_anchor(), false);
		let call = g
			.refs()
			.find(|r| r.kind == b"calls" && r.call_name == b"free".to_vec())
			.expect("field call");
		assert_eq!(call.confidence, b"resolved".to_vec());
		let target_kinds: Vec<&[u8]> = call.target.as_view().segments().map(|s| s.kind).collect();
		assert!(target_kinds.contains(&b"field".as_slice()));
	}

	#[test]
	fn extract_untyped_field_call_is_method_call_fact() {
		let src = "int run(void *o) { return ((obj *)o)->count; }\nvoid go(void *h) { handler(h)->emit(1); }\n";
		let g = extract_default("d.c", src, &make_anchor(), false);
		let dynamic = g
			.refs()
			.find(|r| r.kind == b"method_call" && r.call_name == b"emit".to_vec())
			.expect("untyped fn-pointer call kept as method_call fact");
		assert_eq!(dynamic.confidence, b"name_match".to_vec());
	}

	#[test]
	fn extract_local_type_flow_respects_nested_shadowing() {
		let src = r#"
typedef struct first { void (*run)(void); } first;
typedef struct second { void (*stop)(void); } second;
int use(first *value) {
	value->run();
	{ second *value; value->stop(); }
	value->run();
	return 0;
}
"#;
		let graph = extract_default("flow.c", src, &make_anchor(), false);
		let calls = graph
			.refs()
			.filter(|reference| reference.kind == b"calls")
			.collect::<Vec<_>>();
		assert_eq!(
			calls
				.iter()
				.filter(|reference| {
					reference
						.target
						.as_view()
						.segments()
						.last()
						.is_some_and(|segment| segment.name == b"run")
				})
				.count(),
			2,
		);
		assert!(calls.iter().any(|reference| {
			reference
				.target
				.as_view()
				.segments()
				.last()
				.is_some_and(|segment| segment.name == b"stop")
		}));
	}

	#[test]
	fn extract_global_and_macro_value_reads_are_accounted() {
		let src =
			"#define MAX_LEN 10\nint count;\nint run(void) { count = MAX_LEN; return count; }\n";
		let graph = extract_default("reads.c", src, &make_anchor(), false);
		let read_targets = graph
			.refs()
			.filter(|reference| reference.kind == b"reads")
			.filter_map(|reference| {
				reference
					.target
					.as_view()
					.segments()
					.last()
					.map(|segment| segment.name.to_vec())
			})
			.collect::<Vec<_>>();
		assert!(read_targets.iter().any(|name| name == b"MAX_LEN"));
		assert!(read_targets.iter().any(|name| name == b"count"));
	}

	#[test]
	fn extract_shallow_skips_param_and_local_defs() {
		let src = "int run(int x) { int y = 1; return x + y; }\n";
		let g = extract_default("main.c", src, &make_anchor(), false);
		assert!(
			g.defs().all(|d| d.kind != b"param" && d.kind != b"local"),
			"shallow extraction must not emit param/local defs"
		);
	}

	#[test]
	fn extract_deep_emits_param_and_local_defs() {
		let src = "int run(int x) { int y = 1; return x + y; }\n";
		let g = extract_default("main.c", src, &make_anchor(), true);
		let param = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"main")
			.segment(b"func", b"run(x:int)")
			.segment(b"param", b"x")
			.build();
		let local = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"main")
			.segment(b"func", b"run(x:int)")
			.segment(b"local", b"y")
			.build();
		assert!(g.contains(&param));
		assert!(g.contains(&local));
	}

	#[test]
	fn extract_defs_inside_preproc_conditionals() {
		let src = "#ifdef __linux__\nstatic int only_linux(void) { return 1; }\n#else\nstatic int fallback(void) { return 2; }\n#endif\n";
		let g = extract_default("port.c", src, &make_anchor(), false);
		let names: Vec<Vec<u8>> = g
			.defs()
			.filter(|d| d.kind == b"func")
			.map(|d| d.moniker.as_view().segments().last().unwrap().name.to_vec())
			.collect();
		assert!(names.contains(&b"only_linux()".to_vec()));
		assert!(names.contains(&b"fallback()".to_vec()));
	}

	#[test]
	fn extract_defs_inside_parser_recovery_nodes() {
		let src = "#ifndef RECOVERY_H\n#define RECOVERY_H\nBROKEN(\n#define RECOVERED_VALUE 1\n)\n#endif\n";
		let graph = extract_default("recovery.h", src, &make_anchor(), false);

		assert!(graph.defs().any(|definition| {
			definition.kind == b"const"
				&& definition
					.moniker
					.as_view()
					.segments()
					.last()
					.is_some_and(|segment| segment.name == b"RECOVERED_VALUE")
		}));
		assert!(!graph.defs().any(|definition| {
			definition
				.moniker
				.as_view()
				.segments()
				.last()
				.is_some_and(|segment| segment.name == b"RECOVERY_H")
		}));
	}

	#[test]
	fn extract_defs_inside_conditional_cpp_linkage_block() {
		let src = "#if defined(__cplusplus)\nextern \"C\" {\n#endif\n#define PUBLIC_API 1\ntypedef struct api_state api_state;\n#if defined(__cplusplus)\n}\n#endif\n";
		let graph = extract_default("api.h", src, &make_anchor(), false);
		let names = graph
			.defs()
			.filter_map(|definition| {
				definition
					.moniker
					.as_view()
					.segments()
					.last()
					.map(|segment| segment.name.to_vec())
			})
			.collect::<Vec<_>>();

		assert!(names.contains(&b"PUBLIC_API".to_vec()));
		assert!(names.contains(&b"api_state".to_vec()));
	}

	#[test]
	fn extract_generated_union_across_line_directives() {
		let src = r#"
#if ! defined YYSTYPE
typedef union YYSTYPE
#line 233 "gram.y"
{
	int ival;
	void *node;
}
#line 1425 "gram.c"
YYSTYPE;
#endif
int read_value(YYSTYPE *value) { return value->ival; }
"#;
		let g = extract_default("gram.c", src, &make_anchor(), true);
		let union = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"gram")
			.segment(b"struct", b"YYSTYPE")
			.build();
		let field = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"gram")
			.segment(b"struct", b"YYSTYPE")
			.segment(b"field", b"ival")
			.build();
		assert!(
			g.contains(&union),
			"generated union expected; defs: {:?}",
			g.def_monikers()
		);
		assert!(
			g.contains(&field),
			"generated union field expected; defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn extract_prototype_in_header_is_a_func_def() {
		let src = "struct list;\nstruct list *listCreate(void);\n";
		let g = extract_default("list.h", src, &make_anchor(), false);
		let proto = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"list.h")
			.segment(b"func", b"listCreate()")
			.build();
		assert!(g.contains(&proto));
	}

	#[test]
	fn extract_function_pointer_variable_is_var_not_func() {
		let src = "int (*handler)(int) = 0;\n";
		let g = extract_default("h.c", src, &make_anchor(), false);
		let var = MonikerBuilder::new()
			.project(b"app")
			.segment(b"lang", b"c")
			.segment(b"module", b"h")
			.segment(b"var", b"handler")
			.build();
		assert!(g.contains(&var), "defs: {:?}", g.def_monikers());
		assert!(g.defs().all(|d| d.kind != b"func"));
	}

	#[test]
	fn extract_shallow_function_pointer_call_remains_accounted() {
		let source = "int run(void) { int (*handler)(void) = 0; return handler(); }\n";
		let graph = extract_default("main.c", source, &make_anchor(), false);
		let call = graph
			.refs()
			.find(|reference| {
				reference.kind == b"calls" && reference.call_name == b"handler".to_vec()
			})
			.expect("function-pointer call fact");
		assert_eq!(call.confidence, b"name_match".to_vec());
		let target = call
			.target
			.as_view()
			.segments()
			.last()
			.expect("local target");
		assert_eq!(target.kind, b"local");
		assert_eq!(target.name, b"handler");
	}
}
