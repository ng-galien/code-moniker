-- SQL / PL-pgSQL extraction smoke test (phase 1: DDL via pg_parse_query +
-- top-level call refs via raw_expression_tree_walker).

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(18);

SELECT has_function('extract_plpgsql'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_plpgsql(text, text, moniker, boolean) is exposed');

-- Module moniker = anchor + dir: segments + module:basename.

WITH g AS (
	SELECT extract_plpgsql(
		'db/functions/plan/create_plan.sql',
		'',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT is(graph_root(g)::text,
	'esac+moniker://app/dir:db/dir:functions/dir:plan/module:create_plan',
	'empty source still yields the file-as-module moniker')
FROM g;

-- CREATE FUNCTION public.bar(int, text): full type signature in the
-- moniker so PG's same-name same-arity overloads (min(int) vs min(text))
-- don't collide. Types also mirrored on the `signature` column.

WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'CREATE FUNCTION public.bar(a int, b text) RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/module:foo/schema:public/function:bar(int4,text)'::moniker,
		'qualified function moniker carries schema + full type signature') AS r1,
	is((SELECT signature FROM graph_defs(g) WHERE kind = 'function'),
		'int4,text',
		'function signature column lists parameter types') AS r2
FROM g;

-- Unqualified function: no schema segment, arity 0 → empty parens.

WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'CREATE FUNCTION bar() RETURNS void LANGUAGE sql AS $$ $$;',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/module:foo/function:bar()'::moniker,
		'unqualified function omits the schema segment') AS r3,
	is((SELECT count(*)::int FROM graph_defs(g) WHERE kind = 'function'),
		1,
		'one function def per CREATE FUNCTION') AS r4
FROM g;

-- Same-name same-arity overloads coexist with full-type monikers.

WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'CREATE FUNCTION m(x int) RETURNS int LANGUAGE sql AS $$ SELECT x $$;'
		|| E' CREATE FUNCTION m(x text) RETURNS text LANGUAGE sql AS $$ SELECT x $$;',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT is((SELECT count(*)::int FROM graph_defs(g) WHERE kind = 'function'),
	2,
	'overloads with different types both land in the graph') AS r5
FROM g;

-- CREATE TABLE → kind=class, schema in moniker.

WITH g AS (
	SELECT extract_plpgsql(
		'schema.sql',
		E'CREATE TABLE esac.module_t (id uuid PRIMARY KEY);',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/module:schema/schema:esac/class:module_t'::moniker,
		'CREATE TABLE emits a class def under its schema') AS r6,
	is((SELECT kind::text FROM graph_defs(g) WHERE kind = 'class' LIMIT 1),
		'class',
		'table def kind is class') AS r7
FROM g;

-- CREATE VIEW → kind=interface; calls inside the SELECT body are
-- emitted as `calls` refs with arity-only target and unresolved
-- confidence (raw_parser cannot infer argument types at a call site).

WITH g AS (
	SELECT extract_plpgsql(
		'schema.sql',
		E'CREATE VIEW v AS SELECT esac.foo() FROM t;',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/module:schema/interface:v'::moniker,
		'CREATE VIEW emits an interface def') AS r8,
	ok(EXISTS (SELECT 1 FROM graph_refs(g)
	           WHERE kind = 'calls'
	             AND target = 'esac+moniker://app/module:schema/schema:esac/function:foo()'::moniker
	             AND confidence = 'unresolved'),
		'CREATE VIEW body emits unresolved calls ref to esac.foo()') AS r9
FROM g;

-- Top-level SELECT call: bar(1, 2) → calls ref with arity 2.

WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'SELECT public.bar(1, 2);',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(EXISTS (SELECT 1 FROM graph_refs(g)
	           WHERE kind = 'calls'
	             AND target = 'esac+moniker://app/module:foo/schema:public/function:bar(2)'::moniker),
		'top-level SELECT emits qualified arity-only calls ref') AS r10,
	is((SELECT confidence FROM graph_refs(g) WHERE kind = 'calls' LIMIT 1),
		'unresolved',
		'top-level call confidence is unresolved (types unknown at call site)') AS r11
FROM g;

-- Unqualified top-level call: no schema segment in target.

WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		E'SELECT bar();',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT ok(EXISTS (SELECT 1 FROM graph_refs(g)
	         WHERE kind = 'calls'
	           AND target = 'esac+moniker://app/module:foo/function:bar()'::moniker),
	'unqualified top-level call target omits schema') AS r12
FROM g;

-- Phase 2: PL/pgSQL body extraction. The bison parser is vendored
-- under vendor/plpgsql/ and compiled into our .dylib via build.rs,
-- so this works portably on macOS and Linux without depending on
-- plpgsql.so's hidden internal symbols.

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
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/module:foo/function:outer_fn(int4)'::moniker,
		'plpgsql function def is emitted') AS r14,
	ok(EXISTS (SELECT 1 FROM graph_refs(g) r
	           WHERE r.kind = 'calls'
	             AND r.target = 'esac+moniker://app/module:foo/schema:esac/function:inner_fn(1)'::moniker),
		'PERFORM in plpgsql body emits calls ref to qualified target') AS r15,
	ok(EXISTS (SELECT 1 FROM graph_refs(g) r
	           WHERE r.kind = 'calls'
	             AND r.target = 'esac+moniker://app/module:foo/function:other_fn()'::moniker),
		'IF branch in plpgsql body picks up nested PERFORM call') AS r16
FROM g;

-- Empty source: no defs beyond the module root.

WITH g AS (
	SELECT extract_plpgsql(
		'foo.sql',
		'',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT is((SELECT count(*)::int FROM graph_defs(g)),
	1,
	'empty source emits only the module root') AS r13
FROM g;

SELECT * FROM finish();

ROLLBACK;
