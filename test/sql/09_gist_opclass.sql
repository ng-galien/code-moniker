-- GiST opclass on `moniker`. Inner-page signatures are the LCP of all
-- leaves below; consistent reduces `=`, `<@`, `@>` to byte-prefix tests.
-- The test asserts both correctness (rows match the no-index baseline)
-- and that the planner picks an Index/Bitmap Index Scan when seqscan
-- is forced off.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(13);

-- Build a small table of monikers that exercises ancestry chains.
CREATE TEMP TABLE m (
	id  int  PRIMARY KEY,
	mon moniker NOT NULL
);

INSERT INTO m VALUES
	(1, 'esac://app/src/lib'),
	(2, 'esac://app/src/lib#Lib#'),
	(3, 'esac://app/src/lib#Lib#go().'),
	(4, 'esac://app/src/app'),
	(5, 'esac://app/src/app/main()'),
	(6, 'esac://other/foo'),
	(7, 'esac://other/foo/Bar#'),
	(8, 'esac://app/src/lib#Other#');

CREATE INDEX moniker_gist_idx ON m USING gist (mon);

-- ANALYZE on `moniker` columns is currently broken (typanalyze quirk
-- tracked in TODO.md). enable_seqscan = off forces the index path.
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

-- Strategy 3 (=) ----------------------------------------------------------

SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon = 'esac://app/src/lib#Lib#'::moniker$$,
		'moniker_gist_idx'),
	'= uses moniker_gist_idx');

SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon = 'esac://app/src/lib#Lib#'::moniker$$,
		'Index Scan'),
	'= produces an Index/Bitmap Index Scan node');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon = 'esac://app/src/lib#Lib#'::moniker),
	ARRAY[2]::int[],
	'= matches exactly the equal moniker');

-- Strategy 8 (@>) : key is ancestor of query --------------------------------

SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon @> 'esac://app/src/lib#Lib#go().'::moniker$$,
		'moniker_gist_idx'),
	'@> uses moniker_gist_idx');

SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon @> 'esac://app/src/lib#Lib#go().'::moniker$$,
		'Index Scan'),
	'@> produces an Index/Bitmap Index Scan node');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon @> 'esac://app/src/lib#Lib#go().'::moniker),
	ARRAY[1, 2, 3]::int[],
	'@> finds every ancestor including the query itself');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon @> 'esac://app/src/app/main()'::moniker),
	ARRAY[4, 5]::int[],
	'@> on a different branch picks only that branch');

-- Strategy 10 (<@) : key is descendant of query ----------------------------

SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon <@ 'esac://app/src/lib'::moniker$$,
		'moniker_gist_idx'),
	'<@ uses moniker_gist_idx');

SELECT ok(
	plan_uses(
		$$SELECT id FROM m WHERE mon <@ 'esac://app/src/lib'::moniker$$,
		'Index Scan'),
	'<@ produces an Index/Bitmap Index Scan node');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon <@ 'esac://app/src/lib'::moniker),
	ARRAY[1, 2, 3, 8]::int[],
	'<@ finds the moniker itself and every descendant');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon <@ 'esac://app'::moniker),
	ARRAY[1, 2, 3, 4, 5, 8]::int[],
	'<@ at project root finds all monikers in that project');

SELECT is(
	(SELECT array_agg(id ORDER BY id)
	   FROM m WHERE mon <@ 'esac://other'::moniker),
	ARRAY[6, 7]::int[],
	'<@ at the other project returns disjoint set');

-- Bigger insert: force a real page split so picksplit / union actually run.
INSERT INTO m
	SELECT 100 + g, ('esac://bulk/p' || g)::moniker
	  FROM generate_series(1, 500) g;

SELECT is(
	(SELECT count(*)::int
	   FROM m WHERE mon <@ 'esac://bulk'::moniker),
	500,
	'<@ on a large bulk-loaded subtree returns the full set');

SELECT * FROM finish();

ROLLBACK;
