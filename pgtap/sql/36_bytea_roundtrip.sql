BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(9);

-- moniker bytea roundtrip
WITH m AS (SELECT 'code+moniker://app/lang:ts/module:foo'::moniker AS v)
SELECT is(
	moniker_from_bytea(moniker_to_bytea(v)),
	v,
	'moniker → bytea → moniker is identity'
) FROM m;

WITH m AS (SELECT 'code+moniker://app/lang:ts/module:foo'::moniker AS v)
SELECT cmp_ok(
	octet_length(moniker_to_bytea(v)),
	'>',
	0,
	'moniker_to_bytea produces non-empty bytes'
) FROM m;

SELECT throws_ok(
	$$SELECT moniker_from_bytea('\x00'::bytea)$$,
	NULL,
	NULL,
	'moniker_from_bytea rejects bad bytes'
);

-- code_graph bytea roundtrip
WITH g AS (
	SELECT graph_add_def(
		graph_create('code+moniker://app/lang:ts/module:foo'::moniker, 'module'),
		'code+moniker://app/lang:ts/module:foo/class:Bar'::moniker,
		'class',
		'code+moniker://app/lang:ts/module:foo'::moniker,
		100,
		200
	) AS v
)
SELECT is(
	graph_defs(code_graph_from_bytea(code_graph_to_bytea(v))),
	graph_defs(v),
	'code_graph → bytea → code_graph preserves defs'
) FROM g;

WITH g AS (
	SELECT graph_add_def(
		graph_create('code+moniker://app/lang:ts/module:foo'::moniker, 'module'),
		'code+moniker://app/lang:ts/module:foo/class:Bar'::moniker,
		'class',
		'code+moniker://app/lang:ts/module:foo'::moniker,
		100,
		200
	) AS v
)
SELECT cmp_ok(
	octet_length(code_graph_to_bytea(v)),
	'>=',
	12, -- header is 12 bytes minimum
	'code_graph_to_bytea produces at least a header'
) FROM g;

-- Byte-identity: the bytea bytes ARE the Datum bytes. Roundtrip via bytea
-- column must not change graph_defs output.
CREATE TEMP TABLE staging (id int, graph_bytes bytea);
INSERT INTO staging (id, graph_bytes)
SELECT 1, code_graph_to_bytea(
	graph_add_def(
		graph_create('code+moniker://app/lang:ts/module:foo'::moniker, 'module'),
		'code+moniker://app/lang:ts/module:foo/class:Bar'::moniker,
		'class',
		'code+moniker://app/lang:ts/module:foo'::moniker,
		100,
		200
	)
);

SELECT is(
	(SELECT count(*)::int FROM graph_defs(code_graph_from_bytea((SELECT graph_bytes FROM staging WHERE id = 1)))),
	2,
	'staging table → from_bytea recovers 2 defs (root + class)'
);

-- COPY BINARY workflow: bytea column in/out
SELECT lives_ok(
	$$
		CREATE TEMP TABLE staging_copy (id int, graph_bytes bytea);
		INSERT INTO staging_copy SELECT 1, code_graph_to_bytea(
			graph_create('code+moniker://app/lang:ts/module:foo'::moniker, 'module')
		);
		COPY staging_copy TO '/tmp/code_moniker_bytea_copy.bin' WITH (FORMAT BINARY);
		TRUNCATE staging_copy;
		COPY staging_copy FROM '/tmp/code_moniker_bytea_copy.bin' WITH (FORMAT BINARY);
	$$,
	'COPY BINARY of bytea column round-trips a code_graph'
);

SELECT is(
	(SELECT count(*)::int FROM graph_defs(code_graph_from_bytea((SELECT graph_bytes FROM staging_copy WHERE id = 1)))),
	1,
	'after COPY BINARY roundtrip, decoded graph has 1 def (root)'
);

SELECT * FROM finish();
ROLLBACK;
