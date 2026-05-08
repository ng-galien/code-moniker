
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(4);

CREATE TEMP TABLE m (id int PRIMARY KEY, mon moniker NOT NULL);

INSERT INTO m VALUES
	(1, 'pcm+moniker://app/path:src/path:lib'),
	(2, 'pcm+moniker://app/path:src/path:lib/class:Lib'),
	(3, 'pcm+moniker://app/path:src/path:lib/class:Lib/method:go()'),
	(4, 'pcm+moniker://app/path:src/path:app'),
	(5, 'pcm+moniker://app/path:src/path:app/function:main()'),
	(6, 'pcm+moniker://other/path:foo'),
	(7, 'pcm+moniker://app/path:src/path:lib/class:Other');

SELECT ok(
	'pcm+moniker://app/path:src/path:lib'::moniker
	  < 'pcm+moniker://app/path:src/path:lib/class:Lib/method:go()'::moniker,
	'parent < descendant via byte-lex (v2 tree-friendly)');

SELECT ok(
	'pcm+moniker://app/path:src/path:lib/class:Lib/method:looooooooooooong()'::moniker
	  < 'pcm+moniker://app/path:src/path:lib/class:Other'::moniker,
	'long descendant stays inside parent range — does not leapfrog next sibling');

SELECT is(
	(SELECT array_agg(id ORDER BY id) FROM m
	  WHERE mon >= 'pcm+moniker://app'::moniker
	    AND mon <  'pcm+moniker://app/zzzzz:zzzzzzzzzzzzzz'::moniker),
	ARRAY[1, 2, 3, 4, 5, 7]::int[],
	'btree range query bounds the app project sub-tree');

SELECT is(
	(SELECT array_agg(id ORDER BY id) FROM m
	  WHERE mon >= 'pcm+moniker://app/path:src/path:lib'::moniker
	    AND mon <  'pcm+moniker://app/path:src/path:lib/zzzzz:zzzzzzzzzzzzzz'::moniker),
	ARRAY[1, 2, 3, 7]::int[],
	'btree range query bounds the lib sub-tree');

SELECT * FROM finish();

ROLLBACK;
