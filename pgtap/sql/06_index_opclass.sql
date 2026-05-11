
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(11);


SELECT has_function('moniker_cmp'::name, ARRAY['moniker','moniker'],
	'moniker_cmp(moniker, moniker) is exposed');

SELECT has_function('moniker_hash'::name, ARRAY['moniker'],
	'moniker_hash(moniker) is exposed');


SELECT is(
	(SELECT array_agg(m::text ORDER BY m)
	   FROM (VALUES
	     ('code+moniker://app/path:c'::moniker),
	     ('code+moniker://app/path:a'::moniker),
	     ('code+moniker://app/path:b'::moniker)
	   ) AS t(m)),
	ARRAY['code+moniker://app/path:a', 'code+moniker://app/path:b', 'code+moniker://app/path:c']::text[],
	'ORDER BY moniker uses the btree opclass');

SELECT ok(
	'code+moniker://app/path:main'::moniker < 'code+moniker://app/path:main/class:Foo'::moniker,
	'parent < child via btree');

SELECT ok(
	NOT ('code+moniker://app/class:Foo'::moniker > 'code+moniker://app/class:Foo'::moniker),
	'reflexive: moniker is not strictly greater than itself');


SELECT is(
	(SELECT count(DISTINCT m)::int
	   FROM (VALUES
	     ('code+moniker://app/path:a'::moniker),
	     ('code+moniker://app/path:a'::moniker),
	     ('code+moniker://app/path:b'::moniker)
	   ) AS t(m)),
	2,
	'DISTINCT moniker uses the hash opclass');


CREATE TEMP TABLE module (
	id    text       PRIMARY KEY,
	graph code_graph NOT NULL
);
CREATE INDEX module_defs_gin ON module USING gin (graph_def_monikers(graph));
CREATE INDEX module_refs_gin ON module USING gin (graph_ref_targets(graph));

SELECT pass('GIN on graph_def_monikers(graph) created');
SELECT pass('GIN on graph_ref_targets(graph) created');

INSERT INTO module VALUES
	('lib', extract_typescript('src/lib.ts',
		'export class Lib { go() { return 1; } }',
		'code+moniker://app'::moniker)),
	('app', extract_typescript('src/app.ts',
		'import { Lib } from "./lib";',
		'code+moniker://app'::moniker));

SELECT is(
	(SELECT id FROM module
	  WHERE graph_def_monikers(graph) @> ARRAY['code+moniker://app/lang:ts/dir:src/module:lib/class:Lib'::moniker]),
	'lib',
	'graph_def_monikers @> ARRAY[m] resolves the owning module');

SELECT is(
	(SELECT array_agg(id ORDER BY id) FROM module
	  WHERE graph_ref_targets(graph) @> ARRAY['code+moniker://app/lang:ts/dir:src/module:lib/path:Lib'::moniker]),
	ARRAY['app']::text[],
	'graph_ref_targets @> ARRAY[m] finds every importer');

SELECT is(
	(SELECT array_agg(graph_root(graph)::text ORDER BY graph_root(graph))
	   FROM module),
	ARRAY['code+moniker://app/lang:ts/dir:src/module:app', 'code+moniker://app/lang:ts/dir:src/module:lib']::text[],
	'ORDER BY on a moniker-returning expression');

SELECT * FROM finish();

ROLLBACK;
