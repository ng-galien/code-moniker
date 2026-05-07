-- v2 layout dropped the fixed-offset seg_count, so byte-lex order is
-- now strictly tree-friendly: parent < every descendant < every later
-- sibling. That makes a sub-tree range query (`m >= ancestor AND m <
-- ancestor||sentinel`) well-defined on the plain btree opclass — a
-- cheaper alternative to GiST `<@` for ancestor-bounded scans.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(4);

CREATE TEMP TABLE m (id int PRIMARY KEY, mon moniker NOT NULL);

INSERT INTO m VALUES
	(1, 'esac+moniker://app/path:src/path:lib'),
	(2, 'esac+moniker://app/path:src/path:lib/class:Lib'),
	(3, 'esac+moniker://app/path:src/path:lib/class:Lib/method:go()'),
	(4, 'esac+moniker://app/path:src/path:app'),
	(5, 'esac+moniker://app/path:src/path:app/function:main()'),
	(6, 'esac+moniker://other/path:foo'),
	(7, 'esac+moniker://app/path:src/path:lib/class:Other');

-- Tree-lex invariant: parent comes before every descendant.
SELECT ok(
	'esac+moniker://app/path:src/path:lib'::moniker
	  < 'esac+moniker://app/path:src/path:lib/class:Lib/method:go()'::moniker,
	'parent < descendant via byte-lex (v2 tree-friendly)');

-- Tree-lex invariant: descendant comes before later sibling. This is
-- the case v1 broke when a sibling's name was longer than the parent's.
SELECT ok(
	'esac+moniker://app/path:src/path:lib/class:Lib/method:looooooooooooong()'::moniker
	  < 'esac+moniker://app/path:src/path:lib/class:Other'::moniker,
	'long descendant stays inside parent range — does not leapfrog next sibling');

-- Sub-tree range scan via btree alone. Sentinel `zzzzz:zzz...` is a
-- moniker textually greater than any plausible kind:name extension —
-- relies on `z` being the largest ASCII identifier character we would
-- ever emit. A future tightening could derive the bound from the
-- ancestor's bytes, but text-only is enough at this layer.
SELECT is(
	(SELECT array_agg(id ORDER BY id) FROM m
	  WHERE mon >= 'esac+moniker://app'::moniker
	    AND mon <  'esac+moniker://app/zzzzz:zzzzzzzzzzzzzz'::moniker),
	ARRAY[1, 2, 3, 4, 5, 7]::int[],
	'btree range query bounds the app project sub-tree');

SELECT is(
	(SELECT array_agg(id ORDER BY id) FROM m
	  WHERE mon >= 'esac+moniker://app/path:src/path:lib'::moniker
	    AND mon <  'esac+moniker://app/path:src/path:lib/zzzzz:zzzzzzzzzzzzzz'::moniker),
	ARRAY[1, 2, 3, 7]::int[],
	'btree range query bounds the lib sub-tree');

SELECT * FROM finish();

ROLLBACK;
