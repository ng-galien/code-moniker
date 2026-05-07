-- Compact SCIP-like projection of a moniker (display-only) and its
-- match predicate. The compact form drops kind precision (interface vs
-- class collapse onto `#name#`) — there is no text → moniker parser
-- for it by design.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(13);

-- Surface ------------------------------------------------------------------

SELECT has_function('moniker_compact'::name, ARRAY['moniker'],
	'moniker_compact(moniker) is exposed');
SELECT has_function('match_compact'::name, ARRAY['moniker','text'],
	'match_compact(moniker, text) is exposed');

-- Projection per kind class ------------------------------------------------

SELECT is(
	moniker_compact('esac+moniker://my-app'::moniker),
	'esac://my-app',
	'project-only collapses to bare authority');

SELECT is(
	moniker_compact('esac+moniker://app/path:main/path:com/path:acme'::moniker),
	'esac://app/main/com/acme',
	'path-class kinds use `/` separator');

SELECT is(
	moniker_compact('esac+moniker://app/path:main/class:Foo'::moniker),
	'esac://app/main#Foo#',
	'class-class kinds use `#name#`');

SELECT is(
	moniker_compact('esac+moniker://app/class:Foo/method:bar()'::moniker),
	'esac://app#Foo#bar().',
	'method-class kinds keep the `()` from the v2 name and append `.`');

SELECT is(
	moniker_compact('esac+moniker://app/class:Foo/method:bar(2)'::moniker),
	'esac://app#Foo#bar(2).',
	'method arity disambiguator survives the projection');

SELECT is(
	moniker_compact('esac+moniker://app/class:Foo/field:count'::moniker),
	'esac://app#Foo#count.',
	'term-class kinds use `#name.`');

-- Aliasing across kind precision -------------------------------------------

-- `class:Foo` and `interface:Foo` collapse to the same compact text;
-- intentional trade-off, identity belongs to the canonical typed URI.
SELECT is(
	moniker_compact('esac+moniker://app/class:Foo'::moniker),
	moniker_compact('esac+moniker://app/interface:Foo'::moniker),
	'class and interface alias under compact projection (intentional)');

-- Backtick escape on names containing SCIP-reserved chars. Without
-- escape, `util/test.ts` would collide with a two-segment path chain
-- and break match_compact correctness.
SELECT is(
	moniker_compact('esac+moniker://app/path:`util/test.ts`'::moniker),
	'esac://app/`util/test.ts`',
	'name with `/` is backtick-quoted in the compact form');

SELECT ok(
	match_compact(
		'esac+moniker://app/path:`util/test.ts`'::moniker,
		'esac://app/`util/test.ts`'),
	'match_compact agrees with moniker_compact on escaped names');

-- match_compact -----------------------------------------------------------

SELECT ok(
	match_compact('esac+moniker://app/class:Foo'::moniker, 'esac://app#Foo#'),
	'match_compact returns true for an equal compact projection');

SELECT ok(
	NOT match_compact('esac+moniker://app/class:Foo'::moniker, 'esac://app#Bar#'),
	'match_compact returns false for a non-matching compact text');

SELECT * FROM finish();

ROLLBACK;
