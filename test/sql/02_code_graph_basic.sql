-- Phase 2: code_graph type, constructors, accessors, containment.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(13);

-- Type and surface presence -------------------------------------------------

SELECT has_type('code_graph', 'code_graph type is exposed');

SELECT has_function('graph_create'::name, ARRAY['moniker','text'],
	'graph_create(moniker, text) is exposed');

SELECT has_function('graph_add_def'::name,
	ARRAY['code_graph','moniker','text','moniker'],
	'graph_add_def is exposed');

SELECT has_function('graph_add_ref'::name,
	ARRAY['code_graph','moniker','moniker','text'],
	'graph_add_ref is exposed');

SELECT has_function('graph_root'::name, ARRAY['code_graph'],
	'graph_root is exposed');

-- Build a small graph in a CTE so we can query against it ------------------

WITH g0 AS (
	SELECT graph_create('esac://app/main/Foo'::moniker, 'module') AS g
), g1 AS (
	SELECT graph_add_def(
		g,
		'esac://app/main/Foo#Foo#'::moniker,
		'class',
		'esac://app/main/Foo'::moniker
	) AS g FROM g0
), g2 AS (
	SELECT graph_add_def(
		g,
		'esac://app/main/Foo#Foo#bar().'::moniker,
		'method',
		'esac://app/main/Foo#Foo#'::moniker
	) AS g FROM g1
), gref AS (
	SELECT graph_add_ref(
		g,
		'esac://app/main/Foo#Foo#bar().'::moniker,
		'esac://app/main/Bar#Bar#'::moniker,
		'call'
	) AS g FROM g2
)
SELECT
	is(graph_root(g)::text, 'esac://app/main/Foo',
		'graph_root returns the constructor''s root') AS r1,
	ok(g @> 'esac://app/main/Foo'::moniker,
		'graph contains its root') AS r2,
	ok(g @> 'esac://app/main/Foo#Foo#'::moniker,
		'graph contains an added def') AS r3,
	ok(g @> 'esac://app/main/Foo#Foo#bar().'::moniker,
		'graph contains a nested def') AS r4,
	ok(NOT (g @> 'esac://app/main/Bar#Bar#'::moniker),
		'graph does not contain an unknown moniker (refs are not defs)') AS r5,
	is(array_length(graph_def_monikers(g), 1), 3,
		'graph_def_monikers returns one entry per def') AS r6,
	is(array_length(graph_ref_targets(g), 1), 1,
		'graph_ref_targets returns one entry per ref') AS r7
FROM gref;

-- graph_defs setof -----------------------------------------------------------

WITH g AS (
	SELECT graph_add_def(
		graph_create('esac://app/M'::moniker, 'module'),
		'esac://app/M#Foo#'::moniker,
		'class',
		'esac://app/M'::moniker
	) AS g
)
SELECT is(
	(SELECT count(*)::int FROM g, LATERAL graph_defs(g.g)),
	2,
	'graph_defs emits one row per def (root + added)');

SELECT * FROM finish();

ROLLBACK;
