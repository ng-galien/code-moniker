-- Phase 1: moniker type, equality, accessors.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(11);

-- Type and operator presence -----------------------------------------------

SELECT has_type('moniker', 'moniker type is exposed');

SELECT has_function('moniker_eq'::name, ARRAY['moniker','moniker'],
	'moniker_eq(moniker, moniker) is exposed');

SELECT has_function('project_of'::name, ARRAY['moniker'],
	'project_of(moniker) is exposed');

SELECT has_function('depth'::name, ARRAY['moniker'],
	'depth(moniker) is exposed');

-- Text I/O roundtrip --------------------------------------------------------

SELECT is(
	('esac://my-app'::moniker)::text,
	'esac://my-app',
	'project-only roundtrip');

SELECT is(
	('esac://my-app/main/com/acme#Foo#bar(2).'::moniker)::text,
	'esac://my-app/main/com/acme#Foo#bar(2).',
	'full descriptor chain roundtrip');

-- Equality -----------------------------------------------------------------

SELECT ok(
	'esac://app#Foo#'::moniker = 'esac://app#Foo#'::moniker,
	'identical monikers compare equal');

SELECT ok(
	NOT ('esac://app#Foo#'::moniker = 'esac://app#Bar#'::moniker),
	'different monikers compare unequal');

-- Accessors ----------------------------------------------------------------

SELECT is(
	project_of('esac://my-app/main/com/acme#Foo#'::moniker),
	'my-app',
	'project_of returns the authority');

SELECT is(
	depth('esac://my-app'::moniker),
	0,
	'depth of project-only moniker is 0');

SELECT is(
	depth('esac://my-app/main#Foo#bar().'::moniker),
	3,
	'depth counts every segment regardless of class');

SELECT * FROM finish();

ROLLBACK;
