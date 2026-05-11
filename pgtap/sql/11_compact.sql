
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(13);


SELECT has_function('moniker_compact'::name, ARRAY['moniker'],
	'moniker_compact(moniker) is exposed');
SELECT has_function('match_compact'::name, ARRAY['moniker','text'],
	'match_compact(moniker, text) is exposed');


SELECT is(
	moniker_compact('code+moniker://my-app'::moniker),
	'code://my-app',
	'project-only collapses to bare authority');

SELECT is(
	moniker_compact('code+moniker://app/path:main/path:com/path:acme'::moniker),
	'code://app/main/com/acme',
	'path-class kinds use `/` separator');

SELECT is(
	moniker_compact('code+moniker://app/path:main/class:Foo'::moniker),
	'code://app/main#Foo#',
	'class-class kinds use `#name#`');

SELECT is(
	moniker_compact('code+moniker://app/class:Foo/method:bar()'::moniker),
	'code://app#Foo#bar().',
	'method-class kinds keep the `()` from the v2 name and append `.`');

SELECT is(
	moniker_compact('code+moniker://app/class:Foo/method:bar(2)'::moniker),
	'code://app#Foo#bar(2).',
	'method arity disambiguator survives the projection');

SELECT is(
	moniker_compact('code+moniker://app/class:Foo/field:count'::moniker),
	'code://app#Foo#count.',
	'term-class kinds use `#name.`');


SELECT is(
	moniker_compact('code+moniker://app/class:Foo'::moniker),
	moniker_compact('code+moniker://app/interface:Foo'::moniker),
	'class and interface alias under compact projection (intentional)');

SELECT is(
	moniker_compact('code+moniker://app/path:`util/test.ts`'::moniker),
	'code://app/`util/test.ts`',
	'name with `/` is backtick-quoted in the compact form');

SELECT ok(
	match_compact(
		'code+moniker://app/path:`util/test.ts`'::moniker,
		'code://app/`util/test.ts`'),
	'match_compact agrees with moniker_compact on escaped names');


SELECT ok(
	match_compact('code+moniker://app/class:Foo'::moniker, 'code://app#Foo#'),
	'match_compact returns true for an equal compact projection');

SELECT ok(
	NOT match_compact('code+moniker://app/class:Foo'::moniker, 'code://app#Bar#'),
	'match_compact returns false for a non-matching compact text');

SELECT * FROM finish();

ROLLBACK;
