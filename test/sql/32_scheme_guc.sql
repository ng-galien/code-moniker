BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(8);

SELECT is(
	current_setting('code_moniker.scheme'),
	'code+moniker://',
	'GUC default is code+moniker://');

SELECT is(
	'code+moniker://app/path:foo'::moniker::text,
	'code+moniker://app/path:foo',
	'output uses default scheme');

SET code_moniker.scheme = 'esac+moniker://';

SELECT is(
	'esac+moniker://app/path:foo'::moniker::text,
	'esac+moniker://app/path:foo',
	'after SET, input + output round-trip on the new scheme');

SELECT throws_like(
	$$ SELECT 'code+moniker://app/path:foo'::moniker $$,
	'%moniker parse error%',
	'after SET, the previous scheme is rejected');

SELECT matches(
	moniker_compact('esac+moniker://app/lang:ts/dir:src/module:util'::moniker),
	'^esac://',
	'moniker_compact reads the GUC for the compact scheme prefix');

RESET code_moniker.scheme;

SELECT is(
	current_setting('code_moniker.scheme'),
	'code+moniker://',
	'RESET restores the default');

SELECT is(
	'code+moniker://app/path:foo'::moniker::text,
	'code+moniker://app/path:foo',
	'after RESET, default scheme parses again');

SELECT throws_like(
	$$ SELECT 'esac+moniker://app/path:foo'::moniker $$,
	'%moniker parse error%',
	'after RESET, the custom scheme is rejected');

SELECT * FROM finish();

ROLLBACK;
