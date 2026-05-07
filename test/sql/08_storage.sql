-- Storage shape and ANALYZE compatibility for the manual-varlena
-- `moniker` Datum. Locks two regressions that haunted the cbor-wrapped
-- predecessor: ANALYZE used to fail with `type "moniker" does not exist`,
-- and on-disk size carried 30-50% of cbor framing overhead.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(5);

-- pg_type row shape: variable-length varlena passed by reference, with
-- extended storage. `typanalyze = 0` makes PG fall back to std_typanalyze,
-- which works on any varlena.
SELECT is(typlen,    -1::int2, 'moniker is variable-length')
FROM pg_type WHERE typname = 'moniker';
SELECT is(typbyval,  false,    'moniker is passed by reference')
FROM pg_type WHERE typname = 'moniker';
SELECT is(typstorage, 'x'::"char", 'moniker uses extended storage')
FROM pg_type WHERE typname = 'moniker';

-- Storage size = 4-byte varlena header + canonical encoding payload
-- (1 version + 2 project_len + |project| + 2 seg_count
--  + per segment: 2 kind + 2 arity + 2 seg_len + |seg|).
-- For `esac://app/Foo#`:  4 + 1 + 2 + 3 + 2 + (2+2+2+3) = 21.
SELECT is(
	pg_column_size('esac://app/Foo#'::moniker),
	21,
	'on-disk size matches `varlena_4b + canonical bytes` for one segment'
);

-- ANALYZE on a moniker column used to fail with the cbor wrapper. With
-- the raw varlena Datum, std_typanalyze handles it like bytea.
CREATE TEMP TABLE moniker_analyze_t (m moniker);
INSERT INTO moniker_analyze_t
SELECT compose_child('esac://p'::moniker, 'C' || g, 'class')
FROM generate_series(1, 200) g;
ANALYZE moniker_analyze_t;
SELECT is(
	(SELECT count(*) FROM moniker_analyze_t)::int,
	200,
	'ANALYZE on a moniker column succeeds and the rows survive'
);

SELECT * FROM finish();

ROLLBACK;
