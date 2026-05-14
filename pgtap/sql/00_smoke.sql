
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(2);

SELECT has_function('pcm_version'::name, 'pcm_version() is exposed');
SELECT is(pcm_version(), :'expected_version', 'pcm_version returns the crate version');

SELECT * FROM finish();

ROLLBACK;
