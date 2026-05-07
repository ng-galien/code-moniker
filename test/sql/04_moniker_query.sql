-- Tree-position queries on moniker: containment operators (<@, @>),
-- parent_of / kind_of / path_of accessors, compose_child.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(15);

-- Surface ------------------------------------------------------------------

SELECT has_function('parent_of'::name, ARRAY['moniker'],
	'parent_of(moniker) is exposed');
SELECT has_function('kind_of'::name, ARRAY['moniker'],
	'kind_of(moniker) is exposed');
SELECT has_function('path_of'::name, ARRAY['moniker'],
	'path_of(moniker) is exposed');
SELECT has_function('compose_child'::name, ARRAY['moniker','text','text'],
	'compose_child(moniker, text, text) is exposed');

-- Containment operators ----------------------------------------------------

SELECT ok(
	'esac+moniker://app/path:main'::moniker @> 'esac+moniker://app/path:main/path:com/path:acme'::moniker,
	'@> holds when left is a strict prefix of right');

SELECT ok(
	'esac+moniker://app/path:main/path:com/path:acme'::moniker <@ 'esac+moniker://app/path:main'::moniker,
	'<@ is the inverse of @>');

SELECT ok(
	'esac+moniker://app/path:main'::moniker @> 'esac+moniker://app/path:main'::moniker,
	'@> is reflexive (PG containment convention)');

SELECT ok(
	NOT ('esac+moniker://app/path:main'::moniker @> 'esac+moniker://other/path:main'::moniker),
	'@> requires same project');

SELECT ok(
	NOT ('esac+moniker://app/path:main/path:com'::moniker @> 'esac+moniker://app/path:main/path:edu'::moniker),
	'@> rejects diverging segments');

-- Accessors ----------------------------------------------------------------

SELECT is(
	parent_of('esac+moniker://app/path:main/path:com/path:acme'::moniker)::text,
	'esac+moniker://app/path:main/path:com',
	'parent_of drops the last segment');

SELECT ok(
	parent_of('esac+moniker://app'::moniker) IS NULL,
	'parent_of returns NULL on a project-only moniker');

SELECT is(
	kind_of('esac+moniker://app/path:main/class:Foo'::moniker),
	'class',
	'kind_of returns the kind name of the last segment');

SELECT is(
	path_of('esac+moniker://app/path:main/path:com/path:acme/path:Foo'::moniker),
	ARRAY['main','com','acme','Foo']::text[],
	'path_of returns each segment name in order');

-- Composition --------------------------------------------------------------

SELECT is(
	compose_child('esac+moniker://app/path:main'::moniker, 'path', 'com')::text,
	'esac+moniker://app/path:main/path:com',
	'compose_child appends a typed segment');

SELECT is(
	compose_child('esac+moniker://app/path:main/path:com/path:acme'::moniker, 'class', 'Foo')::text,
	'esac+moniker://app/path:main/path:com/path:acme/class:Foo',
	'compose_child appends a class-kind segment');

SELECT * FROM finish();

ROLLBACK;
