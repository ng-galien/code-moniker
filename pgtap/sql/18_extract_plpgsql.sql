
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(18);

SELECT has_function('extract_plpgsql'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_plpgsql(text, text, moniker, boolean) is exposed');


WITH g AS (
	SELECT extract_plpgsql(
		'db/functions/plan/create_plan.sql',
		'',
		'code+moniker://app'::moniker
	) AS g
)
SELECT is(graph_root(g)::text,
	'code+moniker://app/lang:sql/dir:db/dir:functions/dir:plan/module:create_plan',
	'empty source still yields the file-as-module moniker')
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'CREATE FUNCTION public.bar(a int, b text) RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'code+moniker://app/lang:sql/module:foo/schema:public/function:bar(a:int4,b:text)'::moniker,
		'qualified function moniker carries schema + name:type slots') AS r1,
	is((SELECT signature FROM graph_defs(g) WHERE kind = 'function'),
		'a:int4,b:text',
		'function signature column lists name:type slots') AS r2
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'CREATE FUNCTION bar() RETURNS void LANGUAGE sql AS $$ $$;',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'code+moniker://app/lang:sql/module:foo/function:bar()'::moniker,
		'unqualified function omits the schema segment') AS r3,
	is((SELECT count(*)::int FROM graph_defs(g) WHERE kind = 'function'),
		1,
		'one function def per CREATE FUNCTION') AS r4
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'CREATE FUNCTION m(x int) RETURNS int LANGUAGE sql AS $$ SELECT x $$;'
		|| E' CREATE FUNCTION m(x text) RETURNS text LANGUAGE sql AS $$ SELECT x $$;',
		'code+moniker://app'::moniker
	) AS g
)
SELECT is((SELECT count(*)::int FROM graph_defs(g) WHERE kind = 'function'),
	2,
	'overloads with different types both land in the graph') AS r5
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'schema.sql',
		E'CREATE TABLE esac.module_t (id uuid PRIMARY KEY);',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'code+moniker://app/lang:sql/module:schema/schema:esac/table:module_t'::moniker,
		'CREATE TABLE emits a table def under its schema') AS r6,
	is((SELECT kind::text FROM graph_defs(g) WHERE kind = 'table' LIMIT 1),
		'table',
		'table def kind is table') AS r7
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'schema.sql',
		E'CREATE VIEW v AS SELECT esac.foo() FROM t;',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'code+moniker://app/lang:sql/module:schema/view:v'::moniker,
		'CREATE VIEW emits a view def') AS r8,
	ok(EXISTS (SELECT 1 FROM graph_refs(g)
	           WHERE kind = 'calls'
	             AND target = 'code+moniker://app/lang:sql/module:schema/schema:esac/function:foo'::moniker
	             AND confidence = 'name_match'),
		'CREATE VIEW body emits name-match calls ref to esac.foo()') AS r9
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'SELECT public.bar(1, 2);',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	ok(EXISTS (SELECT 1 FROM graph_refs(g)
	           WHERE kind = 'calls'
	             AND target = 'code+moniker://app/lang:sql/module:foo/schema:public/function:bar'::moniker),
		'top-level SELECT emits qualified name-only calls ref') AS r10,
	is((SELECT confidence FROM graph_refs(g) WHERE kind = 'calls' LIMIT 1),
		'name_match',
		'top-level call confidence is name_match (types unknown at call site)') AS r11
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'SELECT bar();',
		'code+moniker://app'::moniker
	) AS g
)
SELECT ok(EXISTS (SELECT 1 FROM graph_refs(g)
	         WHERE kind = 'calls'
	           AND target = 'code+moniker://app/lang:sql/module:foo/function:bar'::moniker),
	'unqualified top-level call target omits schema') AS r12
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'CREATE FUNCTION outer_fn(x int) RETURNS void LANGUAGE plpgsql AS $$\n'
		|| E'BEGIN\n'
		|| E'  PERFORM esac.inner_fn(x);\n'
		|| E'  IF x > 0 THEN\n'
		|| E'    PERFORM other_fn();\n'
		|| E'  END IF;\n'
		|| E'END;\n'
		|| E'$$;',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'code+moniker://app/lang:sql/module:foo/function:outer_fn(x:int4)'::moniker,
		'plpgsql function def carries name:type slots') AS r14,
	ok(EXISTS (SELECT 1 FROM graph_refs(g) r
	           WHERE r.kind = 'calls'
	             AND r.target = 'code+moniker://app/lang:sql/module:foo/schema:esac/function:inner_fn'::moniker),
		'PERFORM in plpgsql body emits calls ref to qualified name-only target') AS r15,
	ok(EXISTS (SELECT 1 FROM graph_refs(g) r
	           WHERE r.kind = 'calls'
	             AND r.target = 'code+moniker://app/lang:sql/module:foo/function:other_fn'::moniker),
		'IF branch in plpgsql body picks up nested PERFORM call (name-only)') AS r16
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		'',
		'code+moniker://app'::moniker
	) AS g
)
SELECT is((SELECT count(*)::int FROM graph_defs(g)),
	1,
	'empty source emits only the module root') AS r13
FROM g;

SELECT * FROM finish();

ROLLBACK;
