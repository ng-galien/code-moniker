
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(13);

CREATE TEMP TABLE m (
	id  int  PRIMARY KEY,
	mon moniker NOT NULL
);

INSERT INTO m VALUES
	(1, 'code+moniker://app/path:src/path:lib'),
	(2, 'code+moniker://app/path:src/path:lib/class:Lib'),
	(3, 'code+moniker://app/path:src/path:lib/class:Lib/method:go()'),
	(4, 'code+moniker://app/path:src/path:app'),
	(5, 'code+moniker://app/path:src/path:app/function:main()'),
	(6, 'code+moniker://other/path:foo'),
	(7, 'code+moniker://other/path:foo/class:Bar'),
	(8, 'code+moniker://app/path:src/path:lib/class:Other');

CREATE INDEX moniker_gist_idx ON m USING gist (mon);

SET LOCAL enable_seqscan = off;

CREATE OR REPLACE FUNCTION plan_uses(qry text, fragment text) RETURNS bool
	LANGUAGE plpgsql AS $$
DECLARE
	line text;
BEGIN
	FOR line IN EXECUTE 'EXPLAIN ' || qry LOOP
		IF strpos(line, fragment) > 0 THEN
			RETURN true;
		END IF;
	END LOOP;
	RETURN false;
END $$;


SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon = 'code+moniker://app/path:src/path:lib/class:Lib'::moniker$$,
		'moniker_gist_idx'),
	'= uses moniker_gist_idx');

SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon = 'code+moniker://app/path:src/path:lib/class:Lib'::moniker$$,
		'Index Scan'),
	'= produces an Index/Bitmap Index Scan node');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon = 'code+moniker://app/path:src/path:lib/class:Lib'::moniker),
	ARRAY[2]::int[],
	'= matches exactly the equal moniker');


SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon @> 'code+moniker://app/path:src/path:lib/class:Lib/method:go()'::moniker$$,
		'moniker_gist_idx'),
	'@> uses moniker_gist_idx');

SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon @> 'code+moniker://app/path:src/path:lib/class:Lib/method:go()'::moniker$$,
		'Index Scan'),
	'@> produces an Index/Bitmap Index Scan node');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon @> 'code+moniker://app/path:src/path:lib/class:Lib/method:go()'::moniker),
	ARRAY[1, 2, 3]::int[],
	'@> finds every ancestor including the query itself');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon @> 'code+moniker://app/path:src/path:app/function:main()'::moniker),
	ARRAY[4, 5]::int[],
	'@> on a different branch picks only that branch');


SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon <@ 'code+moniker://app/path:src/path:lib'::moniker$$,
		'moniker_gist_idx'),
	'<@ uses moniker_gist_idx');

SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon <@ 'code+moniker://app/path:src/path:lib'::moniker$$,
		'Index Scan'),
	'<@ produces an Index/Bitmap Index Scan node');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon <@ 'code+moniker://app/path:src/path:lib'::moniker),
	ARRAY[1, 2, 3, 8]::int[],
	'<@ finds the moniker itself and every descendant');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon <@ 'code+moniker://app'::moniker),
	ARRAY[1, 2, 3, 4, 5, 8]::int[],
	'<@ at project root finds all monikers in that project');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon <@ 'code+moniker://other'::moniker),
	ARRAY[6, 7]::int[],
	'<@ at the other project returns disjoint set');

INSERT INTO m
	SELECT 100 + g, ('code+moniker://bulk/path:p' || g)::moniker
	  FROM generate_series(1, 500) g;

SELECT is(
	(SELECT count(*)::int
	   FROM m WHERE mon <@ 'code+moniker://bulk'::moniker),
	500,
	'<@ on a large bulk-loaded subtree returns the full set');

SELECT * FROM finish();

ROLLBACK;
