
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(18);

SELECT has_function('extract_plpgsql'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_plpgsql(text, text, moniker, boolean) is exposed');


WITH g AS (
	SELECT extract_plpgsql(
		'db/functions/plan/create_plan.sql',
		'',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT is(graph_root(g)::text,
	'pcm+moniker://app/lang:sql/dir:db/dir:functions/dir:plan/module:create_plan',
	'empty source still yields the file-as-module moniker')
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'CREATE FUNCTION public.bar(a int, b text) RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:sql/module:foo/schema:public/function:bar(int4,text)'::moniker,
		'qualified function moniker carries schema + full type signature') AS r1,
	is((SELECT signature FROM graph_defs(g) WHERE kind = 'function'),
		'int4,text',
		'function signature column lists parameter types') AS r2
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'CREATE FUNCTION bar() RETURNS void LANGUAGE sql AS $$ $$;',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:sql/module:foo/function:bar()'::moniker,
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
		'pcm+moniker://app'::moniker
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
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:sql/module:schema/schema:esac/table:module_t'::moniker,
		'CREATE TABLE emits a table def under its schema') AS r6,
	is((SELECT kind::text FROM graph_defs(g) WHERE kind = 'table' LIMIT 1),
		'table',
		'table def kind is table') AS r7
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'schema.sql',
		E'CREATE VIEW v AS SELECT esac.foo() FROM t;',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:sql/module:schema/view:v'::moniker,
		'CREATE VIEW emits a view def') AS r8,
	ok(EXISTS (SELECT 1 FROM graph_refs(g)
	           WHERE kind = 'calls'
	             AND target = 'pcm+moniker://app/lang:sql/module:schema/schema:esac/function:foo()'::moniker
	             AND confidence = 'unresolved'),
		'CREATE VIEW body emits unresolved calls ref to esac.foo()') AS r9
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'SELECT public.bar(1, 2);',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(EXISTS (SELECT 1 FROM graph_refs(g)
	           WHERE kind = 'calls'
	             AND target = 'pcm+moniker://app/lang:sql/module:foo/schema:public/function:bar(2)'::moniker),
		'top-level SELECT emits qualified arity-only calls ref') AS r10,
	is((SELECT confidence FROM graph_refs(g) WHERE kind = 'calls' LIMIT 1),
		'unresolved',
		'top-level call confidence is unresolved (types unknown at call site)') AS r11
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'SELECT bar();',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT ok(EXISTS (SELECT 1 FROM graph_refs(g)
	         WHERE kind = 'calls'
	           AND target = 'pcm+moniker://app/lang:sql/module:foo/function:bar()'::moniker),
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
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:sql/module:foo/function:outer_fn(int4)'::moniker,
		'plpgsql function def is emitted') AS r14,
	ok(EXISTS (SELECT 1 FROM graph_refs(g) r
	           WHERE r.kind = 'calls'
	             AND r.target = 'pcm+moniker://app/lang:sql/module:foo/schema:esac/function:inner_fn(1)'::moniker),
		'PERFORM in plpgsql body emits calls ref to qualified target') AS r15,
	ok(EXISTS (SELECT 1 FROM graph_refs(g) r
	           WHERE r.kind = 'calls'
	             AND r.target = 'pcm+moniker://app/lang:sql/module:foo/function:other_fn()'::moniker),
		'IF branch in plpgsql body picks up nested PERFORM call') AS r16
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		'',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT is((SELECT count(*)::int FROM graph_defs(g)),
	1,
	'empty source emits only the module root') AS r13
FROM g;

SELECT * FROM finish();

ROLLBACK;
