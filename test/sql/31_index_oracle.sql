-- Oracle test for the index opclasses.
--
-- Each scalar operator on `moniker` (=, <@, @>, ?=) runs through two
-- helpers that set GUCs locally to force the planner into either an
-- indexed scan or a sequential scan, then both results are compared.
-- An index that returns wrong rows or skips matching ones would be
-- silently green under the existing per-operator assertion tests
-- (which only check shape, not equivalence with the ground truth).

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(22);

-- Helper to verify a query is genuinely served by an index under the
-- planner state we put it in. Returns true if the EXPLAIN plan
-- mentions an Index/Bitmap scan; false otherwise. Used by the sanity
-- assertions at the bottom — without it, the oracle would happily
-- compare two seq scans against each other and report green.
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

-- A diverse-enough corpus that the planner actually picks the
-- requested scan strategy. Two projects to exercise the project
-- boundary, multiple tree depths to exercise <@/@> with prefixes,
-- and typed callables paired with arity-only versions to exercise
-- ?= bare-callable matching.
CREATE TEMP TABLE oracle_data(m moniker);
INSERT INTO oracle_data VALUES
	-- project "app", lang:ts
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
	-- project "app", lang:java
	('pcm+moniker://app/lang:java'),
	('pcm+moniker://app/lang:java/package:com'),
	('pcm+moniker://app/lang:java/package:com/package:acme'),
	('pcm+moniker://app/lang:java/package:com/package:acme/class:User'),
	('pcm+moniker://app/lang:java/package:com/package:acme/class:User/method:findById(String)'),
	('pcm+moniker://app/lang:java/package:com/package:acme/class:User/method:findById(1)'),
	-- project "app", lang:sql
	('pcm+moniker://app/lang:sql/schema:public/function:create_plan(uuid,text)'),
	('pcm+moniker://app/lang:sql/schema:public/function:create_plan(2)'),
	('pcm+moniker://app/lang:sql/schema:public/table:plan'),
	-- project "other"
	('pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper'),
	('pcm+moniker://other/lang:ts/dir:src/module:util/class:Helper/method:run(number)'),
	-- root-only
	('pcm+moniker://app'),
	('pcm+moniker://other');

-- Pad the corpus with synthetic rows so the planner has a cost
-- incentive to pick the index even under enable_seqscan = off.
-- Without the padding, postgres treats the seq scan as cheap enough
-- to ignore the off hint on a 20-row table.
INSERT INTO oracle_data
	SELECT ('pcm+moniker://pad/lang:ts/dir:src/module:m' || g::text)::moniker
	FROM generate_series(1, 300) g;

CREATE INDEX oracle_btree ON oracle_data USING btree (m);
CREATE INDEX oracle_gist  ON oracle_data USING gist  (m);

ANALYZE oracle_data;

-- Helpers. SET LOCAL persists for the rest of the transaction, so
-- every helper restores the four scan-related GUCs to its desired
-- target state on entry — otherwise calling oracle_seq before
-- oracle_idx would leave index/bitmap/indexonly off when oracle_idx
-- runs, and the planner would fall back to a (penalised) seq scan
-- silently equating index and seq results.
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


-- =========================================================
-- =  (btree)
-- =========================================================

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


-- =========================================================
-- <@ (descendant — gist)
-- =========================================================

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


-- =========================================================
-- @> (ancestor — gist)
-- =========================================================

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


-- =========================================================
-- ?= (bind_match — gist)
-- =========================================================

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


-- =========================================================
-- Cross-operator combinations (planner picks index for both)
-- =========================================================

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


-- =========================================================
-- Sanity: under enable_seqscan = off, the planner must actually pick
-- the index. Otherwise the oracle would silently compare two
-- sequential scans and the equality test would prove nothing about
-- the index correctness.
-- =========================================================

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
