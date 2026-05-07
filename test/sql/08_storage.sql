-- Storage shape and ANALYZE compatibility for the manual-varlena
-- `moniker` Datum.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(5);

SELECT is(typlen,    -1::int2, 'moniker is variable-length')
FROM pg_type WHERE typname = 'moniker';
SELECT is(typbyval,  false,    'moniker is passed by reference')
FROM pg_type WHERE typname = 'moniker';
SELECT is(typstorage, 'x'::"char", 'moniker uses extended storage')
FROM pg_type WHERE typname = 'moniker';

-- Storage size = 4-byte varlena header + canonical v2 payload:
-- 1 version + 2 project_len + |project| + per segment: 2 kind_len + |kind| + 2 name_len + |name|.
-- For `esac+moniker://app/class:Foo`:
--   4 + 1 + 2 + 3 ("app") + (2 + 5 ("class") + 2 + 3 ("Foo")) = 22.
SELECT is(
	pg_column_size('esac+moniker://app/class:Foo'::moniker),
	22,
	'on-disk size matches `varlena_4b + canonical v2 bytes`'
);

CREATE TEMP TABLE moniker_analyze_t (m moniker);
INSERT INTO moniker_analyze_t
SELECT compose_child('esac+moniker://p'::moniker, 'class', 'C' || g)
FROM generate_series(1, 200) g;
ANALYZE moniker_analyze_t;
SELECT is(
	(SELECT count(*) FROM moniker_analyze_t)::int,
	200,
	'ANALYZE on a moniker column succeeds and the rows survive'
);

SELECT * FROM finish();

ROLLBACK;
