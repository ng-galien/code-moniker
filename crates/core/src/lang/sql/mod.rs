mod body;
mod canonicalize;
mod kinds;
mod sdk_pipeline;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;

use crate::lang::KindSpec;

#[derive(Clone, Debug, Default)]
pub struct Presets {
	pub external_schemas: Vec<String>,
}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	sdk_pipeline::extract(uri, source, anchor, deep, presets)
}

pub struct Lang;

const DEF_KINDS: &[&str] = &[
	"function",
	"procedure",
	"view",
	"table",
	"column",
	"constraint",
	"trigger",
	"type",
	"schema",
];

const DEF_KIND_SPECS: &[KindSpec] = &[
	KindSpec::new("schema", Shape::Namespace, 10, "schema"),
	KindSpec::new("table", Shape::Type, 20, "table"),
	KindSpec::new("view", Shape::Type, 21, "view"),
	KindSpec::new("type", Shape::Type, 22, "type"),
	KindSpec::new("column", Shape::Value, 30, "column"),
	KindSpec::new("constraint", Shape::Value, 31, "constraint"),
	KindSpec::new("trigger", Shape::Value, 32, "trigger"),
	KindSpec::new("function", Shape::Callable, 40, "function"),
	KindSpec::new("procedure", Shape::Callable, 41, "procedure"),
];

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "sql";
	const ALLOWED_KINDS: &'static [&'static str] = DEF_KINDS;
	const KIND_SPECS: &'static [KindSpec] = DEF_KIND_SPECS;
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &[];

	fn file_root(uri: &str, anchor: &Moniker) -> Option<Moniker> {
		Some(canonicalize::compute_module_moniker(anchor, uri))
	}

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
			.map(|d| crate::core::uri::to_uri(&d.moniker, &Default::default()))
			.collect()
	}

	fn ref_targets(g: &CodeGraph) -> Vec<String> {
		g.refs()
			.map(|r| crate::core::uri::to_uri(&r.target, &Default::default()))
			.collect()
	}

	fn relation_edges(g: &CodeGraph, kind: &[u8]) -> Vec<(String, String)> {
		g.refs()
			.filter(|reference| reference.kind == kind)
			.map(|reference| {
				(
					crate::core::uri::to_uri(
						&g.def_at(reference.source).moniker,
						&Default::default(),
					),
					crate::core::uri::to_uri(&reference.target, &Default::default()),
				)
			})
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
	fn overloads_with_different_types_both_land() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION m(x int) RETURNS int LANGUAGE sql AS $$ SELECT x $$;\
			 CREATE FUNCTION m(x text) RETURNS text LANGUAGE sql AS $$ SELECT x $$;",
		);
		assert_eq!(g.defs().filter(|d| d.kind == b"function").count(), 2);
	}

	#[test]
	fn top_level_select_emits_qualified_call() {
		let g = run("foo.sql", "SELECT public.bar(1, 2);");
		assert!(
			ref_targets(&g).iter().any(|t| t
				== "code+moniker://app/lang:sql/module:foo/schema:public/function:bar(int4,int4)"),
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
			crate::core::uri::to_uri(&defs[0].moniker, &Default::default()),
			"code+moniker://app/lang:sql/dir:db/dir:functions/dir:plan/module:create_plan"
		);
	}

	#[test]
	fn nested_calls_preserve_unknown_argument_slots() {
		let g = run("foo.sql", "SELECT f(g(a, b));");
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:f(_)"),
			"outer call f should preserve one unknown slot, got refs: {:?}",
			ref_targets(&g)
		);
		assert!(
			ref_targets(&g)
				.iter()
				.any(|t| t == "code+moniker://app/lang:sql/module:foo/function:g(_,_)"),
			"inner call g should preserve two unknown slots, got refs: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn call_argument_types_come_from_casts_literals_and_parameters() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION public.wrapper(p_id uuid, p_enabled bool) RETURNS int LANGUAGE sql AS $$ SELECT public.choose(p_id, 42::bigint, p_enabled, 'x'::text, NULL) $$;",
		);
		assert!(
			ref_targets(&g).iter().any(|target| target
				== "code+moniker://app/lang:sql/module:foo/schema:public/function:choose(uuid,int8,bool,text,_)") ,
			"typed target missing from {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn casts_inside_expressions_do_not_type_the_whole_argument() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION public.wrapper(p_id int) RETURNS int LANGUAGE sql AS $$ SELECT public.choose(p_id = 1::int, (p_id = 1)::text) $$;",
		);
		assert!(
			ref_targets(&g).iter().any(|target| target
				== "code+moniker://app/lang:sql/module:foo/schema:public/function:choose(_,text)"),
			"an inner cast must not type its enclosing expression: {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn named_call_arguments_preserve_names_and_types() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION public.wrapper(p_id uuid) RETURNS int LANGUAGE sql AS $$ SELECT public.choose(label => 'x'::text, value => p_id) $$;",
		);
		assert!(
			ref_targets(&g).iter().any(|target| target
				== "code+moniker://app/lang:sql/module:foo/schema:public/function:choose(label:text,value:uuid)"),
			"named typed target missing from {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn unquoted_parameter_names_are_canonical_in_definitions_and_calls() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION public.choose(Value int) RETURNS int LANGUAGE sql AS $$ SELECT Value $$; SELECT public.choose(value => 1::int);",
		);
		assert!(def_monikers(&g).iter().any(|definition| definition
			== "code+moniker://app/lang:sql/module:foo/schema:public/function:choose(value:int4)"));
		assert!(ref_targets(&g).iter().any(|target| target
			== "code+moniker://app/lang:sql/module:foo/schema:public/function:choose(value:int4)"));
	}

	#[test]
	fn static_function_search_path_qualifies_unqualified_calls() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION public.wrapper(p_id uuid) RETURNS int LANGUAGE sql SET search_path = jobs, pg_temp AS $$ SELECT refresh(p_id) $$;",
		);
		assert!(
			ref_targets(&g).iter().any(|target| target
				== "code+moniker://app/lang:sql/module:foo/schema:jobs/function:refresh(uuid)"),
			"search-path-qualified target missing from {:?}",
			ref_targets(&g)
		);
	}

	#[test]
	fn dynamic_or_catalog_search_path_does_not_claim_one_schema() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION public.from_role(p_id uuid) RETURNS int LANGUAGE sql SET search_path = \"$user\", jobs, pg_temp AS $$ SELECT refresh(p_id) $$; CREATE FUNCTION public.from_catalog(p_id uuid) RETURNS int LANGUAGE sql SET search_path = pg_catalog, jobs, pg_temp AS $$ SELECT refresh(p_id) $$;",
		);
		let refresh_targets = ref_targets(&g)
			.into_iter()
			.filter(|target| target.contains("function:refresh"))
			.collect::<Vec<_>>();
		assert_eq!(refresh_targets.len(), 2, "got {refresh_targets:?}");
		assert!(
			refresh_targets
				.iter()
				.all(|target| !target.contains("schema:jobs")),
			"dynamic/catalog search paths must stay unqualified: {refresh_targets:?}"
		);
	}

	#[test]
	fn duplicate_routine_keeps_the_first_search_path_with_the_first_body() {
		let g = run(
			"foo.sql",
			"CREATE OR REPLACE FUNCTION public.wrapper(p_id uuid) RETURNS int LANGUAGE sql SET search_path = first_schema, pg_temp AS $$ SELECT refresh(p_id) $$; CREATE OR REPLACE FUNCTION public.wrapper(p_id uuid) RETURNS int LANGUAGE sql SET search_path = second_schema, pg_temp AS $$ SELECT refresh(p_id) $$;",
		);
		let refresh_targets = ref_targets(&g)
			.into_iter()
			.filter(|target| target.contains("function:refresh"))
			.collect::<Vec<_>>();
		assert_eq!(refresh_targets.len(), 1, "got {refresh_targets:?}");
		assert!(refresh_targets[0].contains("schema:first_schema"));
	}

	#[test]
	fn duplicate_routine_keeps_the_last_callable_arity_metadata() {
		let g = run(
			"foo.sql",
			"CREATE OR REPLACE FUNCTION public.wrapper(p_id uuid) RETURNS int LANGUAGE sql AS $$ SELECT 1 $$; CREATE OR REPLACE FUNCTION public.wrapper(p_id uuid DEFAULT NULL) RETURNS int LANGUAGE sql AS $$ SELECT 2 $$;",
		);
		let wrapper = g
			.defs()
			.find(|definition| definition.call_name == b"wrapper")
			.expect("wrapper definition");
		assert_eq!(wrapper.call_arity, Some(0));
	}

	#[test]
	fn ddl_in_sql_body_does_not_emit_a_definition_with_an_invalid_parent() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION public.wrapper() RETURNS void LANGUAGE sql AS $$ CREATE TABLE public.inner_table(id int); $$;",
		);
		assert_eq!(
			g.defs()
				.filter(|definition| definition.kind == b"function")
				.count(),
			1
		);
		assert_eq!(
			g.defs()
				.filter(|definition| definition.kind == b"table")
				.count(),
			0
		);
	}

	#[test]
	fn parser_recovery_keywords_do_not_become_calls() {
		let g = run(
			"scratch.sql",
			"SELECT * FROM (SELECT id FROM things WHERE id =) broken; SELECT DISTINCT id FROM things;",
		);
		let names = g
			.refs()
			.filter(|reference| reference.kind == b"calls")
			.map(|reference| String::from_utf8_lossy(&reference.call_name).into_owned())
			.collect::<Vec<_>>();
		assert!(
			names
				.iter()
				.all(|name| !matches!(name.as_str(), "from" | "distinct" | "as" | "is" | "any")),
			"parser recovery emitted SQL keywords as calls: {names:?}"
		);
	}

	#[test]
	fn comment_def_bytes_are_a_real_comment_in_outer_source() {
		let src = r#"CREATE OR REPLACE FUNCTION foo.bar(
  p_a uuid,
  p_b text
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = foo, pg_temp
AS $$
DECLARE
  v_x text;
BEGIN
  -- real comment, do not lose
  v_x := 'hello';
END;
$$;
"#;
		let g = run("fixture.sql", src);
		for d in g.defs().filter(|d| d.kind == b"comment") {
			let (s, e) = d.position.expect("comment def must have a position");
			let slice = &src.as_bytes()[s as usize..e as usize];
			assert!(
				slice.starts_with(b"--") || slice.starts_with(b"/*"),
				"comment def bytes {s}..{e} are not a real comment: {:?}",
				std::str::from_utf8(slice).unwrap_or("?")
			);
		}
	}

	#[test]
	fn function_param_emits_uses_type_with_pg_catalog_target() {
		let g = run(
			"pkg.sql",
			"CREATE FUNCTION f(x int, y text) RETURNS bigint LANGUAGE sql AS $$ SELECT 1 $$;",
		);
		let int_target = "code+moniker://app/sdk:sql/path:pg_catalog/path:int4";
		let text_target = "code+moniker://app/sdk:sql/path:pg_catalog/path:text";
		let bigint_target = "code+moniker://app/sdk:sql/path:pg_catalog/path:int8";
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
	fn user_defined_types_are_definitions_and_qualified_type_targets() {
		let g = run(
			"types.sql",
			"CREATE TYPE app.order_state AS ENUM ('new', 'done'); CREATE DOMAIN app.order_code AS text; CREATE FUNCTION app.accept(value app.order_state) RETURNS app.order_code LANGUAGE sql AS $$ SELECT value::text $$;",
		);
		let definitions = def_monikers(&g);
		assert!(definitions.iter().any(|definition| definition
			== "code+moniker://app/lang:sql/module:types/schema:app/type:order_state"));
		assert!(definitions.iter().any(|definition| definition
			== "code+moniker://app/lang:sql/module:types/schema:app/type:order_code"));
		let targets = ref_targets(&g);
		assert!(targets.iter().any(|target| target
			== "code+moniker://app/lang:sql/module:types/schema:app/type:order_state"));
		assert!(targets.iter().any(|target| target
			== "code+moniker://app/lang:sql/module:types/schema:app/type:order_code"));
	}

	#[test]
	fn quoted_user_type_targets_fold_only_unquoted_identifiers() {
		let g = run(
			"quoted_types.sql",
			r#"
CREATE TYPE Sales."OrderRow" AS (id uuid);
CREATE FUNCTION Sales.load_orders()
RETURNS SETOF Sales."OrderRow"
LANGUAGE sql
AS $$ SELECT NULL::Sales."OrderRow" $$;
"#,
		);
		let targets = ref_targets(&g);
		assert!(
			targets.iter().any(|target| target
				== "code+moniker://app/lang:sql/module:quoted_types/schema:sales/type:OrderRow"),
			"quoted components preserve case while unquoted components fold: {targets:?}"
		);
		assert!(
			targets
				.iter()
				.all(|target| !target.contains("schema:`SETOF Sales`")
					&& !target.contains("type:%22OrderRow%22"))
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
	fn corpus_builtin_families_are_external() {
		let g = run(
			"pkg.sql",
			"SELECT chr(65), json_build_object('id', 1), array_append(ARRAY[1], 2), row_number() OVER (), gen_random_uuid(), plainto_tsquery('code moniker'), quote_ident('table'), pg_tablespace_location(1), txid_current(), jsonb_array_length('[]'), array_upper(ARRAY[1], 1), ts_rank_cd(to_tsvector('code'), plainto_tsquery('code')), inet_client_addr(), split_part('a.b', '.', 1);",
		);
		let calls = g
			.refs()
			.filter(|reference| reference.kind == b"calls")
			.collect::<Vec<_>>();
		assert_eq!(calls.len(), 16, "got {calls:?}");
		assert!(
			calls
				.iter()
				.all(|reference| reference.confidence == b"external")
		);
	}

	#[test]
	fn callable_metadata_uses_sql_argument_nodes() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION Public.Combine(a int, b numeric(10, 2)) RETURNS int LANGUAGE sql AS $$ SELECT 1 $$; SELECT PUBLIC.combine(1, 2); SELECT nested(2, 3);",
		);
		let definition = g
			.defs()
			.find(|def| def.kind == b"function" && def.call_name == b"combine")
			.expect("combine definition");
		assert_eq!(definition.call_arity, Some(2));
		let combine = g
			.refs()
			.find(|reference| reference.call_name == b"combine")
			.expect("combine call");
		let nested = g
			.refs()
			.find(|reference| reference.call_name == b"nested")
			.expect("nested call");
		assert_eq!(combine.call_arity, Some(2));
		assert_eq!(nested.call_arity, Some(2));
	}

	#[test]
	fn callable_metadata_models_defaults_and_out_parameters() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION optional_arg(value int DEFAULT 1) RETURNS int LANGUAGE sql AS $$ SELECT value $$; CREATE FUNCTION parse_type(value text, OUT type_id oid, OUT modifier int) RETURNS record LANGUAGE sql AS $$ SELECT 1, 2 $$;",
		);
		let optional = g
			.defs()
			.find(|def| def.call_name == b"optional_arg")
			.expect("optional_arg definition");
		let parse_type = g
			.defs()
			.find(|def| def.call_name == b"parse_type")
			.expect("parse_type definition");
		assert_eq!(optional.call_arity, Some(0));
		assert_eq!(parse_type.call_arity, Some(1));
	}

	#[test]
	fn variadic_parameters_are_explicit_in_callable_identity() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION concat_all(VARIADIC items text[]) RETURNS text LANGUAGE sql AS $$ SELECT '' $$;",
		);
		assert!(
			def_monikers(&g).iter().any(|definition| definition
				== "code+moniker://app/lang:sql/module:foo/function:concat_all(items:text[]...)"),
			"got defs: {:?}",
			def_monikers(&g)
		);
	}

	#[test]
	fn uppercase_builtin_types_and_catalog_calls_are_external() {
		let g = run(
			"foo.sql",
			"CREATE FUNCTION answer() RETURNS NUMERIC LANGUAGE sql AS $$ SELECT 1 $$; SELECT pg_catalog.current_setting('search_path');",
		);
		let type_ref = g
			.refs()
			.find(|reference| reference.kind == b"uses_type")
			.expect("return type");
		assert_eq!(type_ref.confidence, b"external");
		let catalog_call = g
			.refs()
			.find(|reference| reference.kind == b"calls")
			.expect("catalog call");
		assert_eq!(catalog_call.confidence, b"external");
		assert_eq!(
			crate::core::uri::to_uri(&catalog_call.target, &Default::default()),
			"code+moniker://app/sdk:sql/path:pg_catalog/path:current_setting"
		);
	}

	#[test]
	fn create_procedure_emits_procedure_callable() {
		let src = "CREATE PROCEDURE refresh(value int) LANGUAGE sql AS $$ SELECT 1 $$;";
		let g = run("foo.sql", src);
		let procedure = g
			.defs()
			.find(|def| def.kind == b"procedure")
			.expect("procedure definition");
		assert_eq!(procedure.call_name, b"refresh");
		assert_eq!(procedure.call_arity, Some(1));
	}

	#[test]
	fn call_statements_target_procedures_and_quoted_names_are_canonical() {
		let g = run(
			"foo.sql",
			"CREATE PROCEDURE public.\"RefreshCache\"() LANGUAGE sql AS $$ SELECT 1 $$; CALL public.\"RefreshCache\"(); SELECT PUBLIC.REFRESHCACHE();",
		);
		let procedure_call = g
			.refs()
			.find(|reference| reference.call_name == b"RefreshCache")
			.expect("quoted procedure call");
		assert_eq!(
			procedure_call
				.target
				.as_view()
				.segments()
				.last()
				.unwrap()
				.kind,
			b"procedure"
		);
		let unquoted_call = g
			.refs()
			.find(|reference| reference.call_name == b"refreshcache")
			.expect("unquoted function call");
		assert_eq!(
			unquoted_call
				.target
				.as_view()
				.segments()
				.last()
				.unwrap()
				.kind,
			b"function"
		);
	}

	#[test]
	fn relational_ddl_emits_schemas_columns_constraints_and_column_types() {
		let g = run(
			"relational.sql",
			r#"
CREATE SCHEMA IF NOT EXISTS Sales;
CREATE TABLE Sales."Orders" (
  "ID" uuid CONSTRAINT "Orders_PK" PRIMARY KEY,
  customer_id uuid NOT NULL,
  CONSTRAINT orders_customer_fk
    FOREIGN KEY (customer_id) REFERENCES crm.customers(id)
);
"#,
		);
		let definitions = def_monikers(&g);
		assert!(
			definitions.iter().any(|definition| definition
				== "code+moniker://app/lang:sql/module:relational/schema:sales")
		);
		assert!(definitions.iter().any(|definition| definition
			== "code+moniker://app/lang:sql/module:relational/schema:sales/table:Orders/column:ID"));
		assert!(definitions.iter().any(|definition| definition
			== "code+moniker://app/lang:sql/module:relational/schema:sales/table:Orders/column:customer_id"));
		assert!(definitions.iter().any(|definition| definition
			== "code+moniker://app/lang:sql/module:relational/schema:sales/table:Orders/constraint:Orders_PK"));
		assert!(definitions.iter().any(|definition| definition
			== "code+moniker://app/lang:sql/module:relational/schema:sales/table:Orders/constraint:orders_customer_fk"));
		assert_eq!(
			g.defs()
				.filter(|definition| definition.kind == b"constraint")
				.count(),
			3,
			"named and anonymous column/table constraints must all be definitions"
		);
		let customer = g
			.defs()
			.find(|definition| {
				definition.kind == b"column"
					&& definition
						.moniker
						.as_view()
						.segments()
						.last()
						.is_some_and(|segment| segment.name == b"customer_id")
			})
			.expect("customer_id column");
		assert_eq!(customer.signature, b"uuid");
		let uses_type = relation_edges(&g, b"uses_type");
		assert!(uses_type.iter().any(|(source, target)| {
			source.ends_with("/table:Orders/column:customer_id")
				&& target == "code+moniker://app/sdk:sql/path:pg_catalog/path:uuid"
		}));
		assert!(
			g.defs()
				.filter(|definition| matches!(definition.kind.as_ref(), b"column" | b"constraint"))
				.all(|definition| definition.position.is_some())
		);
	}

	#[test]
	fn foreign_keys_reference_the_target_table_and_available_columns() {
		let g = run(
			"foreign_keys.sql",
			r#"
CREATE TABLE sales.orders (
  customer_id uuid,
  CONSTRAINT orders_customer_fk
    FOREIGN KEY (customer_id) REFERENCES crm.customers(id)
);
"#,
		);
		let edges = relation_edges(&g, b"references");
		let source = "code+moniker://app/lang:sql/module:foreign_keys/schema:sales/table:orders/constraint:orders_customer_fk";
		assert!(edges.iter().any(|(from, to)| {
			from == source
				&& to
					== "code+moniker://app/lang:sql/module:foreign_keys/schema:crm/table:customers"
		}));
		assert!(edges.iter().any(|(from, to)| {
			from == source
				&& to
					== "code+moniker://app/lang:sql/module:foreign_keys/schema:crm/table:customers/column:id"
		}));
		assert!(
			g.refs()
				.filter(|reference| reference.kind == b"references")
				.all(|reference| reference.position.is_some())
		);
	}

	#[test]
	fn triggers_reference_relations_and_call_zero_argument_trigger_functions() {
		let g = run(
			"triggers.sql",
			r#"
CREATE TRIGGER "AuditOrders"
AFTER INSERT OR UPDATE ON Sales."Orders"
FOR EACH ROW EXECUTE FUNCTION audit.log_order('orders');

CREATE CONSTRAINT TRIGGER orders_customer_check
AFTER UPDATE ON Sales."Orders"
FROM crm.customers
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE PROCEDURE Sales.check_customer();

CREATE TRIGGER route_order_update
INSTEAD OF UPDATE ON Sales.open_orders
FOR EACH ROW EXECUTE FUNCTION Sales.route_order_update();
"#,
		);
		let definitions = def_monikers(&g);
		let audit_trigger = "code+moniker://app/lang:sql/module:triggers/schema:sales/table:Orders/trigger:AuditOrders";
		let constraint_trigger = "code+moniker://app/lang:sql/module:triggers/schema:sales/table:Orders/trigger:orders_customer_check";
		let view_trigger = "code+moniker://app/lang:sql/module:triggers/schema:sales/view:open_orders/trigger:route_order_update";
		assert!(
			definitions
				.iter()
				.any(|definition| definition == audit_trigger)
		);
		assert!(
			definitions
				.iter()
				.any(|definition| definition == constraint_trigger)
		);
		assert!(
			definitions
				.iter()
				.any(|definition| definition == view_trigger)
		);

		let references = relation_edges(&g, b"references");
		assert!(references.iter().any(|(source, target)| {
			source == audit_trigger
				&& target == "code+moniker://app/lang:sql/module:triggers/schema:sales/table:Orders"
		}));
		assert!(references.iter().any(|(source, target)| {
			source == constraint_trigger
				&& target
					== "code+moniker://app/lang:sql/module:triggers/schema:crm/table:customers"
		}));
		assert!(references.iter().any(|(source, target)| {
			source == view_trigger
				&& target
					== "code+moniker://app/lang:sql/module:triggers/schema:sales/view:open_orders"
		}));

		let calls = relation_edges(&g, b"calls");
		assert!(calls.iter().any(|(source, target)| {
			source == audit_trigger
				&& target
					== "code+moniker://app/lang:sql/module:triggers/schema:audit/function:log_order()"
		}));
		assert!(calls.iter().any(|(source, target)| {
			source == constraint_trigger
				&& target
					== "code+moniker://app/lang:sql/module:triggers/schema:sales/function:check_customer()"
		}));
		assert!(calls.iter().any(|(source, target)| {
			source == view_trigger
				&& target
					== "code+moniker://app/lang:sql/module:triggers/schema:sales/function:route_order_update()"
		}));
		assert!(
			g.refs()
				.filter(|reference| {
					reference.kind == b"calls"
						&& (reference.call_name == b"log_order"
							|| reference.call_name == b"check_customer"
							|| reference.call_name == b"route_order_update")
				})
				.all(|reference| reference.call_arity == Some(0)),
			"trigger arguments populate TG_ARGV and do not change the trigger function signature"
		);
	}

	#[test]
	fn views_and_routines_read_physical_relations_but_not_ctes_or_aliases() {
		let g = run(
			"queries.sql",
			r#"
CREATE VIEW sales.open_orders AS
WITH recent AS (
  SELECT * FROM sales.orders
)
SELECT r.id
FROM recent r
JOIN crm.customers c ON c.id = r.customer_id;

CREATE FUNCTION sales.customer_orders()
RETURNS SETOF sales.orders
LANGUAGE sql
SET search_path = sales, pg_temp
AS $$ SELECT * FROM orders o JOIN crm.customers c ON c.id = o.customer_id $$;
"#,
		);
		let reads = relation_edges(&g, b"reads");
		assert!(
			reads.iter().any(|(source, target)| {
				source.ends_with("/schema:sales/view:open_orders")
					&& target.ends_with("/schema:sales/table:orders")
			}),
			"missing view -> orders read: {reads:?}"
		);
		assert!(
			reads.iter().any(|(source, target)| {
				source.ends_with("/schema:sales/view:open_orders")
					&& target.ends_with("/schema:crm/table:customers")
			}),
			"missing view -> customers read: {reads:?}"
		);
		assert!(reads.iter().any(|(source, target)| {
			source.contains("/schema:sales/function:customer_orders(")
				&& target.ends_with("/schema:sales/table:orders")
		}));
		assert!(
			reads
				.iter()
				.all(|(_, target)| !target.ends_with("/table:recent")
					&& !target.ends_with("/table:r")
					&& !target.ends_with("/table:c")),
			"CTEs and aliases are not physical relations: {reads:?}"
		);
	}

	#[test]
	fn dml_and_create_table_as_emit_writes_and_select_reads() {
		let g = run(
			"mutations.sql",
			r#"
INSERT INTO audit.events SELECT * FROM sales.orders;
UPDATE sales.orders o SET customer_id = c.id FROM crm.customers c;
DELETE FROM sales.orders o USING crm.customers c;
CREATE TABLE sales.orders_copy AS SELECT * FROM sales.orders;
"#,
		);
		let writes = relation_edges(&g, b"writes");
		assert!(writes.iter().any(|(source, target)| {
			source.ends_with("/module:mutations") && target.ends_with("/schema:audit/table:events")
		}));
		assert!(writes.iter().any(|(source, target)| {
			source.ends_with("/module:mutations") && target.ends_with("/schema:sales/table:orders")
		}));
		assert!(writes.iter().any(|(source, target)| {
			source.ends_with("/module:mutations")
				&& target.ends_with("/schema:sales/table:orders_copy")
		}));
		let reads = relation_edges(&g, b"reads");
		assert!(
			reads
				.iter()
				.any(|(_, target)| { target.ends_with("/schema:sales/table:orders") })
		);
		assert!(
			reads
				.iter()
				.any(|(_, target)| { target.ends_with("/schema:crm/table:customers") })
		);
	}

	#[test]
	fn plpgsql_static_statements_are_relational_but_dynamic_sql_is_not_certain() {
		let g = run(
			"procedures.sql",
			r#"
CREATE PROCEDURE sales.refresh(target_table text)
LANGUAGE plpgsql AS $$
BEGIN
  INSERT INTO audit.events SELECT * FROM sales.orders;
  EXECUTE 'INSERT INTO audit.' || quote_ident(target_table) || ' SELECT * FROM sales.orders';
END;
$$;
"#,
		);
		let writes = relation_edges(&g, b"writes");
		assert_eq!(
			writes
				.iter()
				.filter(|(_, target)| target.ends_with("/schema:audit/table:events"))
				.count(),
			1,
			"only the static statement may claim the concrete write: {writes:?}"
		);
		let reads = relation_edges(&g, b"reads");
		assert_eq!(
			reads
				.iter()
				.filter(|(_, target)| target.ends_with("/schema:sales/table:orders"))
				.count(),
			1,
			"dynamic SQL text must not fabricate a second certain read: {reads:?}"
		);
	}
}
