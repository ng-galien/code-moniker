mod body;
mod canonicalize;
mod kinds;
mod strategy;

use canonicalize::compute_module_moniker;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use crate::lang::canonical_walker::CanonicalWalker;

#[derive(Clone, Debug, Default)]
pub struct Presets {
	pub external_schemas: Vec<String>,
}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	_deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let (def_cap, ref_cap) = CodeGraph::capacity_for_source(source.len());
	let mut graph = CodeGraph::with_capacity(module.clone(), kinds::MODULE, def_cap, ref_cap);
	let tree = strategy::parse(source);
	let strat = strategy::Strategy {
		module: module.clone(),
		source_str: source,
	};
	let walker = CanonicalWalker::new(&strat, source.as_bytes());
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

pub struct Lang;

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "sql";
	const ALLOWED_KINDS: &'static [&'static str] =
		&["function", "procedure", "view", "table", "schema"];
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &[];

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

	fn anchor() -> Moniker {
		MonikerBuilder::new().project(b"app").build()
	}

	fn run(uri: &str, src: &str) -> CodeGraph {
		extract(uri, src, &anchor(), false, &Presets::default())
	}

	fn def_monikers(g: &CodeGraph) -> Vec<String> {
		g.defs()
			.map(|d| crate::core::uri::to_uri(&d.moniker, &Default::default()).unwrap())
			.collect()
	}

	fn ref_targets(g: &CodeGraph) -> Vec<String> {
		g.refs()
			.map(|r| crate::core::uri::to_uri(&r.target, &Default::default()).unwrap())
			.collect()
	}

	#[test]
	fn qualified_function_emits_full_signature() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION public.bar(a int, b text) RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;",
		);
		assert!(
			def_monikers(&g).iter().any(|m| m
				== "code+moniker://app/lang:sql/module:foo/schema:public/function:bar(a:int4,b:text)"),
			"got defs: {:?}",
			def_monikers(&g)
		);
		let func = g
			.defs()
			.find(|d| d.kind == b"function")
			.expect("function def");
		assert_eq!(func.signature, b"a:int4,b:text");
	}

	#[test]
	fn unqualified_function_omits_schema() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION bar() RETURNS void LANGUAGE sql AS $$ $$;",
		);
		assert!(
			def_monikers(&g)
				.iter()
				.any(|m| m == "code+moniker://app/lang:sql/module:foo/function:bar()")
		);
		assert_eq!(g.defs().filter(|d| d.kind == b"function").count(), 1);
	}

	#[test]
	fn overloads_with_different_types_both_land() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION m(x int) RETURNS int LANGUAGE sql AS $$ SELECT x $$;\
			 CREATE FUNCTION m(x text) RETURNS text LANGUAGE sql AS $$ SELECT x $$;",
		);
		assert_eq!(g.defs().filter(|d| d.kind == b"function").count(), 2);
	}

	#[test]
	fn create_table_emits_table_under_schema() {
		let g = run(
			"schema.sql",
			"CREATE TABLE esac.module_t (id uuid PRIMARY KEY);",
		);
		assert!(
			def_monikers(&g).iter().any(
				|m| m == "code+moniker://app/lang:sql/module:schema/schema:esac/table:module_t"
			)
		);
	}

	#[test]
	fn create_view_emits_view_and_call_ref() {
		let g = run("schema.sql", "CREATE VIEW v AS SELECT esac.foo() FROM t;");
		assert!(
			def_monikers(&g)
				.iter()
				.any(|m| m == "code+moniker://app/lang:sql/module:schema/view:v")
		);
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:schema/schema:esac/function:foo"),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn top_level_select_emits_qualified_call() {
		let g = run("foo.sql", "SELECT public.bar(1, 2);");
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/schema:public/function:bar"),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn unqualified_top_level_call_omits_schema() {
		let g = run("foo.sql", "SELECT bar();");
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:bar"),
			"got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn empty_source_yields_only_module_root() {
		let g = run("db/functions/plan/create_plan.sql", "");
		let defs: Vec<_> = g.defs().collect();
		assert_eq!(defs.len(), 1);
		assert_eq!(
			crate::core::uri::to_uri(&defs[0].moniker, &Default::default()).unwrap(),
			"code+moniker://app/lang:sql/dir:db/dir:functions/dir:plan/module:create_plan"
		);
	}

	#[test]
	fn nested_calls_both_emit_name_only_targets() {
		let g = run("foo.sql", "SELECT f(g(a, b));");
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:f"),
			"outer call f should emit name-only target, got refs: {:?}",
			ref_targets(&g)
		);
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:g"),
			"inner call g should emit name-only target, got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn line_comment_emits_comment_def() {
		let g = run(
			"pkg.sql",
			"-- this is a header\nCREATE FUNCTION f() RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;",
		);
		assert_eq!(
			g.defs().filter(|d| d.kind == b"comment").count(),
			1,
			"a single -- line comment must produce one comment def, defs: {:?}",
			g.def_monikers()
		);
	}

	#[test]
	fn function_param_emits_uses_type_with_pg_catalog_target() {
		let g = run(
			"pkg.sql",
			"CREATE FUNCTION f(x int, y text) RETURNS bigint LANGUAGE sql AS $$ SELECT 1 $$;",
		);
		let int_target = "code+moniker://app/external_pkg:pg_catalog/path:int4";
		let text_target = "code+moniker://app/external_pkg:pg_catalog/path:text";
		let bigint_target = "code+moniker://app/external_pkg:pg_catalog/path:int8";
		let targets = ref_targets(&g);
		assert!(
			targets.iter().any(|t| t == int_target),
			"int param must emit uses_type → pg_catalog/path:int4, got: {targets:?}"
		);
		assert!(
			targets.iter().any(|t| t == text_target),
			"text param must emit uses_type → pg_catalog/path:text"
		);
		assert!(
			targets.iter().any(|t| t == bigint_target),
			"bigint return must emit uses_type → pg_catalog/path:int8"
		);
		let uses_type_count = g.refs().filter(|r| r.kind == b"uses_type").count();
		assert!(
			uses_type_count >= 3,
			"expected at least 3 uses_type refs (2 params + 1 return), got {uses_type_count}"
		);
	}

	#[test]
	fn table_column_types_emit_uses_type() {
		let g = run("pkg.sql", "CREATE TABLE t (id int, name text);");
		let int_target = "code+moniker://app/external_pkg:pg_catalog/path:int4";
		let text_target = "code+moniker://app/external_pkg:pg_catalog/path:text";
		let targets = ref_targets(&g);
		assert!(
			targets.iter().any(|t| t == int_target),
			"column type int must emit uses_type → int4"
		);
		assert!(
			targets.iter().any(|t| t == text_target),
			"column type text must emit uses_type → text"
		);
	}

	#[test]
	fn builtin_function_call_carries_external_confidence() {
		let g = run("pkg.sql", "SELECT now();");
		let r = g
			.refs()
			.find(|r| r.kind == b"calls")
			.expect("calls ref for now()");
		assert_eq!(
			r.confidence,
			b"external".to_vec(),
			"builtin functions like now() must be marked external, got {:?}",
			std::str::from_utf8(&r.confidence).unwrap_or("?")
		);
	}

	#[test]
	fn function_def_has_byte_range() {
		let g = run(
			"pkg.sql",
			"CREATE FUNCTION f() RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;",
		);
		let func = g.defs().find(|d| d.kind == b"function").expect("function");
		let (s, e) = func.position.expect("position");
		assert!(s <= e, "start={s} end={e}");
	}
}
