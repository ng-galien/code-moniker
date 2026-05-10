BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(8);

SELECT is(
	current_setting('pg_code_moniker.scheme'),
	'pcm+moniker://',
	'GUC default is pcm+moniker://');

SELECT is(
	'pcm+moniker://app/path:foo'::moniker::text,
	'pcm+moniker://app/path:foo',
	'output uses default scheme');

SET pg_code_moniker.scheme = 'esac+moniker://';

SELECT is(
	'esac+moniker://app/path:foo'::moniker::text,
	'esac+moniker://app/path:foo',
	'after SET, input + output round-trip on the new scheme');

SELECT throws_like(
	$$ SELECT 'pcm+moniker://app/path:foo'::moniker $$,
	'%moniker parse error%',
	'after SET, the previous scheme is rejected');

SELECT matches(
	moniker_compact('esac+moniker://app/lang:ts/dir:src/module:util'::moniker),
	'^esac://',
	'moniker_compact reads the GUC for the compact scheme prefix');

RESET pg_code_moniker.scheme;

SELECT is(
	current_setting('pg_code_moniker.scheme'),
	'pcm+moniker://',
	'RESET restores the default');

SELECT is(
	'pcm+moniker://app/path:foo'::moniker::text,
	'pcm+moniker://app/path:foo',
	'after RESET, default scheme parses again');

SELECT throws_like(
	$$ SELECT 'esac+moniker://app/path:foo'::moniker $$,
	'%moniker parse error%',
	'after RESET, the custom scheme is rejected');

SELECT * FROM finish();

ROLLBACK;
