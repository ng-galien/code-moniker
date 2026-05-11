
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(13);


SELECT has_type('code_graph', 'code_graph type is exposed');

SELECT has_function('graph_create'::name, ARRAY['moniker','text'],
	'graph_create(moniker, text) is exposed');

SELECT has_function('graph_add_def'::name,
	ARRAY['code_graph','moniker','text','moniker','integer','integer'],
	'graph_add_def is exposed');

SELECT has_function('graph_add_ref'::name,
	ARRAY['code_graph','moniker','moniker','text','integer','integer'],
	'graph_add_ref is exposed');

SELECT has_function('graph_root'::name, ARRAY['code_graph'],
	'graph_root is exposed');


WITH g0 AS (
	SELECT graph_create('code+moniker://app/path:main/path:Foo'::moniker, 'module') AS g
), g1 AS (
	SELECT graph_add_def(
		g,
		'code+moniker://app/path:main/path:Foo/class:Foo'::moniker,
		'class',
		'code+moniker://app/path:main/path:Foo'::moniker
	) AS g FROM g0
), g2 AS (
	SELECT graph_add_def(
		g,
		'code+moniker://app/path:main/path:Foo/class:Foo/method:bar()'::moniker,
		'method',
		'code+moniker://app/path:main/path:Foo/class:Foo'::moniker
	) AS g FROM g1
), gref AS (
	SELECT graph_add_ref(
		g,
		'code+moniker://app/path:main/path:Foo/class:Foo/method:bar()'::moniker,
		'code+moniker://app/path:main/path:Bar/class:Bar'::moniker,
		'call'
	) AS g FROM g2
)
SELECT
	is(graph_root(g)::text, 'code+moniker://app/path:main/path:Foo',
		'graph_root returns the constructor''s root') AS r1,
	ok(g @> 'code+moniker://app/path:main/path:Foo'::moniker,
		'graph contains its root') AS r2,
	ok(g @> 'code+moniker://app/path:main/path:Foo/class:Foo'::moniker,
		'graph contains an added def') AS r3,
	ok(g @> 'code+moniker://app/path:main/path:Foo/class:Foo/method:bar()'::moniker,
		'graph contains a nested def') AS r4,
	ok(NOT (g @> 'code+moniker://app/path:main/path:Bar/class:Bar'::moniker),
		'graph does not contain an unknown moniker (refs are not defs)') AS r5,
	is(array_length(graph_def_monikers(g), 1), 3,
		'graph_def_monikers returns one entry per def') AS r6,
	is(array_length(graph_ref_targets(g), 1), 1,
		'graph_ref_targets returns one entry per ref') AS r7
FROM gref;


WITH g AS (
	SELECT graph_add_def(
		graph_create('code+moniker://app/path:M'::moniker, 'module'),
		'code+moniker://app/path:M/class:Foo'::moniker,
		'class',
		'code+moniker://app/path:M'::moniker
	) AS g
)
SELECT is(
	(SELECT count(*)::int FROM g, LATERAL graph_defs(g.g)),
	2,
	'graph_defs emits one row per def (root + added)');

SELECT * FROM finish();

ROLLBACK;
