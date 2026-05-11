
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(15);


SELECT has_function('parent_of'::name, ARRAY['moniker'],
	'parent_of(moniker) is exposed');
SELECT has_function('kind_of'::name, ARRAY['moniker'],
	'kind_of(moniker) is exposed');
SELECT has_function('path_of'::name, ARRAY['moniker'],
	'path_of(moniker) is exposed');
SELECT has_function('compose_child'::name, ARRAY['moniker','text','text'],
	'compose_child(moniker, text, text) is exposed');


SELECT ok(
	'code+moniker://app/path:main'::moniker @> 'code+moniker://app/path:main/path:com/path:acme'::moniker,
	'@> holds when left is a strict prefix of right');

SELECT ok(
	'code+moniker://app/path:main/path:com/path:acme'::moniker <@ 'code+moniker://app/path:main'::moniker,
	'<@ is the inverse of @>');

SELECT ok(
	'code+moniker://app/path:main'::moniker @> 'code+moniker://app/path:main'::moniker,
	'@> is reflexive (PG containment convention)');

SELECT ok(
	NOT ('code+moniker://app/path:main'::moniker @> 'code+moniker://other/path:main'::moniker),
	'@> requires same project');

SELECT ok(
	NOT ('code+moniker://app/path:main/path:com'::moniker @> 'code+moniker://app/path:main/path:edu'::moniker),
	'@> rejects diverging segments');


SELECT is(
	parent_of('code+moniker://app/path:main/path:com/path:acme'::moniker)::text,
	'code+moniker://app/path:main/path:com',
	'parent_of drops the last segment');

SELECT ok(
	parent_of('code+moniker://app'::moniker) IS NULL,
	'parent_of returns NULL on a project-only moniker');

SELECT is(
	kind_of('code+moniker://app/path:main/class:Foo'::moniker),
	'class',
	'kind_of returns the kind name of the last segment');

SELECT is(
	path_of('code+moniker://app/path:main/path:com/path:acme/path:Foo'::moniker),
	ARRAY['main','com','acme','Foo']::text[],
	'path_of returns each segment name in order');


SELECT is(
	compose_child('code+moniker://app/path:main'::moniker, 'path', 'com')::text,
	'code+moniker://app/path:main/path:com',
	'compose_child appends a typed segment');

SELECT is(
	compose_child('code+moniker://app/path:main/path:com/path:acme'::moniker, 'class', 'Foo')::text,
	'code+moniker://app/path:main/path:com/path:acme/class:Foo',
	'compose_child appends a class-kind segment');

SELECT * FROM finish();

ROLLBACK;
