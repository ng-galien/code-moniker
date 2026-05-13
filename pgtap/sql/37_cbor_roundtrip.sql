BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(8);

-- moniker CBOR roundtrip
WITH m AS (SELECT 'code+moniker://app/lang:ts/module:foo'::moniker AS v)
SELECT is(
	moniker_from_cbor(moniker_to_cbor(v)),
	v,
	'moniker → cbor → moniker is identity'
) FROM m;

WITH m AS (SELECT 'code+moniker://app/lang:ts/module:foo'::moniker AS v)
SELECT cmp_ok(
	octet_length(moniker_to_cbor(v)),
	'>',
	0,
	'moniker_to_cbor produces non-empty bytes'
) FROM m;

SELECT throws_ok(
	$$SELECT moniker_from_cbor('\x00'::bytea)$$,
	NULL,
	NULL,
	'moniker_from_cbor rejects bad bytes'
);

-- code_graph CBOR roundtrip preserves defs after decode (validates lazy
-- index rebuild — index isn't serialized, find_def self-heals).
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
	(SELECT count(*)::int FROM graph_defs(code_graph_from_cbor(code_graph_to_cbor(v)))),
	2,
	'code_graph → cbor → code_graph preserves 2 defs (root + class)'
) FROM g;

-- CBOR is structurally distinct from the custom bytea format. They should NOT
-- be byte-identical (different encodings for the same graph).
WITH g AS (
	SELECT graph_create('code+moniker://app/lang:ts/module:foo'::moniker, 'module') AS v
)
SELECT cmp_ok(
	octet_length(code_graph_to_cbor(v)),
	'!=',
	octet_length(code_graph_to_bytea(v)),
	'CBOR and bytea encodings differ in size (distinct formats)'
) FROM g;

-- Polyglot import workflow: produce CBOR externally, stage via bytea + COPY
-- BINARY, hydrate via code_graph_from_cbor.
CREATE TEMP TABLE graph_import (id text, payload bytea);
INSERT INTO graph_import (id, payload)
SELECT 'demo', code_graph_to_cbor(
	graph_add_def(
		graph_create('code+moniker://app/lang:ts/module:foo'::moniker, 'module'),
		'code+moniker://app/lang:ts/module:foo/class:Bar'::moniker,
		'class',
		'code+moniker://app/lang:ts/module:foo'::moniker,
		100,
		200
	)
);

-- Now COPY-BINARY round-trip the bytea column. This is what a polyglot
-- producer would target (their framework emits CBOR bytes per row, packaged
-- as a PGCOPY-binary file).
SELECT lives_ok(
	$$
		COPY graph_import TO '/tmp/code_moniker_cbor_import.bin' WITH (FORMAT BINARY);
		TRUNCATE graph_import;
		COPY graph_import FROM '/tmp/code_moniker_cbor_import.bin' WITH (FORMAT BINARY);
	$$,
	'COPY BINARY round-trip preserves CBOR bytea payload'
);

SELECT is(
	(SELECT id FROM graph_import LIMIT 1),
	'demo',
	'staging row survives COPY BINARY'
);

SELECT is(
	(SELECT count(*)::int FROM graph_defs(code_graph_from_cbor((SELECT payload FROM graph_import WHERE id = 'demo')))),
	2,
	'CBOR payload → code_graph_from_cbor recovers 2 defs after COPY round-trip'
);

SELECT * FROM finish();
ROLLBACK;
