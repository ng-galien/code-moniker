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
	'esac://app/main'::moniker @> 'esac://app/main/com/acme'::moniker,
	'@> holds when left is a strict prefix of right');

SELECT ok(
	'esac://app/main/com/acme'::moniker <@ 'esac://app/main'::moniker,
	'<@ is the inverse of @>');

SELECT ok(
	'esac://app/main'::moniker @> 'esac://app/main'::moniker,
	'@> is reflexive (PG containment convention)');

SELECT ok(
	NOT ('esac://app/main'::moniker @> 'esac://other/main'::moniker),
	'@> requires same project');

SELECT ok(
	NOT ('esac://app/main/com'::moniker @> 'esac://app/main/edu'::moniker),
	'@> rejects diverging segments');

-- Accessors ----------------------------------------------------------------

SELECT is(
	parent_of('esac://app/main/com/acme'::moniker)::text,
	'esac://app/main/com',
	'parent_of drops the last segment');

SELECT ok(
	parent_of('esac://app'::moniker) IS NULL,
	'parent_of returns NULL on a project-only moniker');

SELECT is(
	kind_of('esac://app/main#Foo#'::moniker),
	'type',
	'kind_of returns the canonical kind name of the last segment');

SELECT is(
	path_of('esac://app/main/com/acme/Foo'::moniker),
	ARRAY['main','com','acme','Foo']::text[],
	'path_of returns each segment name in order');

-- Composition --------------------------------------------------------------

SELECT is(
	compose_child('esac://app/main'::moniker, 'com', 'path')::text,
	'esac://app/main/com',
	'compose_child appends a path-class segment');

SELECT is(
	compose_child('esac://app/main/com/acme'::moniker, 'Foo', 'class')::text,
	'esac://app/main/com/acme#Foo#',
	'compose_child appends a Type-class segment with the right punctuation');

SELECT * FROM finish();

ROLLBACK;
