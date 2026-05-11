
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(11);


SELECT has_type('moniker', 'moniker type is exposed');

SELECT has_function('moniker_eq'::name, ARRAY['moniker','moniker'],
	'moniker_eq(moniker, moniker) is exposed');

SELECT has_function('project_of'::name, ARRAY['moniker'],
	'project_of(moniker) is exposed');

SELECT has_function('depth'::name, ARRAY['moniker'],
	'depth(moniker) is exposed');


SELECT is(
	('code+moniker://my-app'::moniker)::text,
	'code+moniker://my-app',
	'project-only roundtrip');

SELECT is(
	('code+moniker://my-app/path:main/path:com/path:acme/class:Foo/method:bar(2)'::moniker)::text,
	'code+moniker://my-app/path:main/path:com/path:acme/class:Foo/method:bar(2)',
	'full descriptor chain roundtrip');


SELECT ok(
	'code+moniker://app/class:Foo'::moniker = 'code+moniker://app/class:Foo'::moniker,
	'identical monikers compare equal');

SELECT ok(
	NOT ('code+moniker://app/class:Foo'::moniker = 'code+moniker://app/class:Bar'::moniker),
	'different monikers compare unequal');


SELECT is(
	project_of('code+moniker://my-app/path:main/path:com/path:acme/class:Foo'::moniker),
	'my-app',
	'project_of returns the authority');

SELECT is(
	depth('code+moniker://my-app'::moniker),
	0,
	'depth of project-only moniker is 0');

SELECT is(
	depth('code+moniker://my-app/path:main/class:Foo/method:bar()'::moniker),
	3,
	'depth counts every segment regardless of kind');

SELECT * FROM finish();

ROLLBACK;
