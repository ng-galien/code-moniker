BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(13);

CREATE TEMP TABLE oracle_arr(id int, ms moniker[]);

INSERT INTO oracle_arr VALUES
	(1, ARRAY[
		'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker,
		'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:run(int)'::moniker
	]),
	(2, ARRAY[
		'pcm+moniker://app/lang:ts/dir:src/module:app/function:main()'::moniker,
		'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker
	]),
	(3, ARRAY[
		'pcm+moniker://app/lang:java/package:com/package:acme/class:User'::moniker
	]),
	(4, ARRAY[]::moniker[]),
	(5, ARRAY[
		'pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper'::moniker
	]);

INSERT INTO oracle_arr
	SELECT g + 100,
	       ARRAY[('pcm+moniker://pad/lang:ts/dir:src/module:m' || g::text)::moniker]
	FROM generate_series(1, 300) g;

CREATE INDEX oracle_arr_gin ON oracle_arr USING gin (ms);

ANALYZE oracle_arr;

CREATE OR REPLACE FUNCTION oracle_seq_id(qs text) RETURNS int[] AS $$
DECLARE r int[];
BEGIN
	SET LOCAL enable_seqscan        = on;
	SET LOCAL enable_indexscan      = off;
	SET LOCAL enable_bitmapscan     = off;
	SET LOCAL enable_indexonlyscan  = off;
	EXECUTE 'SELECT array_agg(id ORDER BY id) FROM (' || qs || ') AS q(id)' INTO r;
	RETURN COALESCE(r, ARRAY[]::int[]);
END $$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION oracle_idx_id(qs text) RETURNS int[] AS $$
DECLARE r int[];
BEGIN
	SET LOCAL enable_seqscan        = off;
	SET LOCAL enable_indexscan      = on;
	SET LOCAL enable_bitmapscan     = on;
	SET LOCAL enable_indexonlyscan  = on;
	EXECUTE 'SELECT array_agg(id ORDER BY id) FROM (' || qs || ') AS q(id)' INTO r;
	RETURN COALESCE(r, ARRAY[]::int[]);
END $$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION oracle_uses_gin(qs text) RETURNS boolean AS $$
DECLARE plan_line text; hit boolean := false;
BEGIN
	SET LOCAL enable_seqscan        = off;
	SET LOCAL enable_indexscan      = on;
	SET LOCAL enable_bitmapscan     = on;
	SET LOCAL enable_indexonlyscan  = on;
	FOR plan_line IN EXECUTE 'EXPLAIN (FORMAT TEXT) ' || qs LOOP
		IF plan_line ~ 'Bitmap (Heap|Index) Scan' THEN
			hit := true;
		END IF;
	END LOOP;
	RETURN hit;
END $$ LANGUAGE plpgsql;


-- @> (array contains)
SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker] $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker] $q$),
	'@> : single-element membership');

SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY[
	    'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker,
	    'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:run(int)'::moniker
	  ] $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY[
	    'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker,
	    'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:run(int)'::moniker
	  ] $q$),
	'@> : multi-element subset');

SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://app/path:never_there'::moniker] $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://app/path:never_there'::moniker] $q$),
	'@> : absent moniker yields empty');

SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper'::moniker] $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper'::moniker] $q$),
	'@> : project boundary respected');

-- && (array overlap)
SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms && ARRAY[
	    'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker,
	    'pcm+moniker://app/lang:ts/dir:src/module:app/function:main()'::moniker
	  ] $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms && ARRAY[
	    'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker,
	    'pcm+moniker://app/lang:ts/dir:src/module:app/function:main()'::moniker
	  ] $q$),
	'&& : overlap with two probes');

SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms && ARRAY[]::moniker[] $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms && ARRAY[]::moniker[] $q$),
	'&& : empty probe set');

SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms && ARRAY['pcm+moniker://nope/x:y'::moniker] $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms && ARRAY['pcm+moniker://nope/x:y'::moniker] $q$),
	'&& : no overlap');

-- combined predicates
SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker]
	    AND id < 10 $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker]
	    AND id < 10 $q$),
	'@> AND id : combined predicate');

SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker]
	     OR ms @> ARRAY['pcm+moniker://app/lang:java/package:com/package:acme/class:User'::moniker] $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker]
	     OR ms @> ARRAY['pcm+moniker://app/lang:java/package:com/package:acme/class:User'::moniker] $q$),
	'@> OR @> : two-key union');

-- array equality (not GIN-served but the result must still match)
SELECT is(
	oracle_idx_id($q$ SELECT id FROM oracle_arr
	  WHERE ms = ARRAY['pcm+moniker://app/lang:java/package:com/package:acme/class:User'::moniker] $q$),
	oracle_seq_id($q$ SELECT id FROM oracle_arr
	  WHERE ms = ARRAY['pcm+moniker://app/lang:java/package:com/package:acme/class:User'::moniker] $q$),
	'= : full-array equality');

-- sanity: GIN must actually serve the queries
SELECT ok(
	oracle_uses_gin($q$ SELECT id FROM oracle_arr
	  WHERE ms @> ARRAY['pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker] $q$),
	'sanity: @> served by GIN');

SELECT ok(
	oracle_uses_gin($q$ SELECT id FROM oracle_arr
	  WHERE ms && ARRAY['pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker] $q$),
	'sanity: && served by GIN');

-- on the canonical shape: graph_def_monikers(graph) wrapped in GIN
CREATE TEMP TABLE oracle_module(id text, graph code_graph);
INSERT INTO oracle_module VALUES
	('lib', extract_typescript('src/lib.ts',
		'export class Lib { go() { return 1; } }',
		'pcm+moniker://app'::moniker)),
	('app', extract_typescript('src/app.ts',
		'import { Lib } from "./lib";',
		'pcm+moniker://app'::moniker));
CREATE INDEX oracle_module_gin ON oracle_module USING gin (graph_def_monikers(graph));
ANALYZE oracle_module;

SELECT is(
	oracle_idx_id($q$ SELECT 0 AS id FROM oracle_module
	  WHERE graph_def_monikers(graph) @> ARRAY[
	    'pcm+moniker://app/lang:ts/dir:src/module:lib/class:Lib'::moniker
	  ] AND id IS NOT NULL $q$),
	oracle_seq_id($q$ SELECT 0 AS id FROM oracle_module
	  WHERE graph_def_monikers(graph) @> ARRAY[
	    'pcm+moniker://app/lang:ts/dir:src/module:lib/class:Lib'::moniker
	  ] AND id IS NOT NULL $q$),
	'graph_def_monikers GIN: @> on the canonical wrapper');

SELECT * FROM finish();

ROLLBACK;
