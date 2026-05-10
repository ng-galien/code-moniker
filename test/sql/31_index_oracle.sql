-- Oracle test: each scalar operator on `moniker` (=, <@, @>, ?=)
-- must return the same rows via the index and via a sequential scan.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(22);

CREATE OR REPLACE FUNCTION oracle_uses_index(qs text) RETURNS boolean AS $$
DECLARE
	plan_line text;
	hit boolean := false;
BEGIN
	SET LOCAL enable_seqscan        = off;
	SET LOCAL enable_indexscan      = on;
	SET LOCAL enable_bitmapscan     = on;
	SET LOCAL enable_indexonlyscan  = on;
	FOR plan_line IN EXECUTE 'EXPLAIN (FORMAT TEXT) ' || qs LOOP
		IF plan_line ~ '(Index Scan|Bitmap (Heap|Index) Scan)' THEN
			hit := true;
		END IF;
	END LOOP;
	RETURN hit;
END
$$ LANGUAGE plpgsql;

CREATE TEMP TABLE oracle_data(m moniker);
INSERT INTO oracle_data VALUES
	('pcm+moniker://app/lang:ts'),
	('pcm+moniker://app/lang:ts/dir:src'),
	('pcm+moniker://app/lang:ts/dir:src/module:util'),
	('pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'),
	('pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:run(number)'),
	('pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:run(2)'),
	('pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:reset()'),
	('pcm+moniker://app/lang:ts/dir:src/module:util/class:Other'),
	('pcm+moniker://app/lang:ts/dir:src/module:app'),
	('pcm+moniker://app/lang:ts/dir:src/module:app/function:main()'),
	('pcm+moniker://app/lang:java'),
	('pcm+moniker://app/lang:java/package:com'),
	('pcm+moniker://app/lang:java/package:com/package:acme'),
	('pcm+moniker://app/lang:java/package:com/package:acme/class:User'),
	('pcm+moniker://app/lang:java/package:com/package:acme/class:User/method:findById(String)'),
	('pcm+moniker://app/lang:java/package:com/package:acme/class:User/method:findById(1)'),
	('pcm+moniker://app/lang:sql/schema:public/function:create_plan(uuid,text)'),
	('pcm+moniker://app/lang:sql/schema:public/function:create_plan(2)'),
	('pcm+moniker://app/lang:sql/schema:public/table:plan'),
	('pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper'),
	('pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper/method:run(number)'),
	('pcm+moniker://app'),
	('pcm+moniker://other');

-- Padding so the planner picks the index over a 20-row seq scan.
INSERT INTO oracle_data
	SELECT ('pcm+moniker://pad/lang:ts/dir:src/module:m' || g::text)::moniker
	FROM generate_series(1, 300) g;

CREATE INDEX oracle_btree ON oracle_data USING btree (m);
CREATE INDEX oracle_gist  ON oracle_data USING gist  (m);

ANALYZE oracle_data;

-- SET LOCAL persists across the whole transaction, so each helper
-- pins all four scan GUCs on entry rather than just toggling one.
CREATE OR REPLACE FUNCTION oracle_seq(qs text) RETURNS moniker[] AS $$
DECLARE
	r moniker[];
BEGIN
	SET LOCAL enable_seqscan        = on;
	SET LOCAL enable_indexscan      = off;
	SET LOCAL enable_bitmapscan     = off;
	SET LOCAL enable_indexonlyscan  = off;
	EXECUTE 'SELECT array_agg(m ORDER BY m) FROM (' || qs || ') AS q(m)' INTO r;
	RETURN COALESCE(r, ARRAY[]::moniker[]);
END
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION oracle_idx(qs text) RETURNS moniker[] AS $$
DECLARE
	r moniker[];
BEGIN
	SET LOCAL enable_seqscan        = off;
	SET LOCAL enable_indexscan      = on;
	SET LOCAL enable_bitmapscan     = on;
	SET LOCAL enable_indexonlyscan  = on;
	EXECUTE 'SELECT array_agg(m ORDER BY m) FROM (' || qs || ') AS q(m)' INTO r;
	RETURN COALESCE(r, ARRAY[]::moniker[]);
END
$$ LANGUAGE plpgsql;


-- = (btree)
SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m = ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m = ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper''::moniker'),
	'= : exact-match present in corpus');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m = ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Missing''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m = ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Missing''::moniker'),
	'= : exact-match absent from corpus');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m = ''pcm+moniker://app''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m = ''pcm+moniker://app''::moniker'),
	'= : project-only moniker');


-- <@ (gist)
SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://app/lang:ts''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://app/lang:ts''::moniker'),
	'<@ : every node under lang:ts');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://app/lang:java/package:com/package:acme/class:User''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://app/lang:java/package:com/package:acme/class:User''::moniker'),
	'<@ : Java class subtree (mix of typed + arity callable methods)');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://app''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://app''::moniker'),
	'<@ : whole project-app subtree');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://other''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://other''::moniker'),
	'<@ : crossing project boundary returns no foreign rows');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://app/lang:python''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://app/lang:python''::moniker'),
	'<@ : empty subtree returns empty');


-- @> (gist)
SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m @> ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:run(number)''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m @> ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:run(number)''::moniker'),
	'@> : every ancestor of a deep TS method');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m @> ''pcm+moniker://app/lang:java/package:com/package:acme/class:User''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m @> ''pcm+moniker://app/lang:java/package:com/package:acme/class:User''::moniker'),
	'@> : every ancestor of a Java class');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m @> ''pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m @> ''pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper''::moniker'),
	'@> : ancestor chain in project-other');


-- ?= (gist)
SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:java/package:com/package:acme/class:User/method:findById(String)''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:java/package:com/package:acme/class:User/method:findById(String)''::moniker'),
	'?= : Java typed-def matches its arity-only call');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:run(2)''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:run(2)''::moniker'),
	'?= : TS arity-only call matches the typed def');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:sql/schema:public/function:create_plan(2)''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:sql/schema:public/function:create_plan(2)''::moniker'),
	'?= : SQL arity-only call matches the typed def');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper''::moniker'),
	'?= : non-callable target matches itself only');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:nope(int)''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper/method:nope(int)''::moniker'),
	'?= : missing bare-name returns empty');

SELECT is(
	oracle_idx('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper/method:run(2)''::moniker'),
	oracle_seq('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper/method:run(2)''::moniker'),
	'?= : project boundary is honoured by the bind_match arm');


-- combined predicates
SELECT is(
	oracle_idx($qs$
		SELECT m FROM oracle_data
		WHERE m <@ 'pcm+moniker://app/lang:ts'::moniker
		  AND m  = 'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker
	$qs$),
	oracle_seq($qs$
		SELECT m FROM oracle_data
		WHERE m <@ 'pcm+moniker://app/lang:ts'::moniker
		  AND m  = 'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker
	$qs$),
	'<@ AND = : combined predicate stays consistent');

SELECT is(
	oracle_idx($qs$
		SELECT m FROM oracle_data
		WHERE m <@ 'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker
		   OR m <@ 'pcm+moniker://app/lang:java/package:com/package:acme/class:User'::moniker
	$qs$),
	oracle_seq($qs$
		SELECT m FROM oracle_data
		WHERE m <@ 'pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper'::moniker
		   OR m <@ 'pcm+moniker://app/lang:java/package:com/package:acme/class:User'::moniker
	$qs$),
	'<@ OR <@ : two-subtree union stays consistent');


-- sanity: the index must actually be picked under enable_seqscan=off
SELECT ok(
	oracle_uses_index('SELECT m FROM oracle_data WHERE m = ''pcm+moniker://app/lang:ts/dir:src/module:util/class:Helper''::moniker'),
	'sanity: = is served by the btree index');

SELECT ok(
	oracle_uses_index('SELECT m FROM oracle_data WHERE m <@ ''pcm+moniker://app/lang:ts''::moniker'),
	'sanity: <@ is served by the gist index');

SELECT ok(
	oracle_uses_index('SELECT m FROM oracle_data WHERE m ?= ''pcm+moniker://app/lang:java/package:com/package:acme/class:User/method:findById(String)''::moniker'),
	'sanity: ?= is served by the gist index');

SELECT * FROM finish();

ROLLBACK;
