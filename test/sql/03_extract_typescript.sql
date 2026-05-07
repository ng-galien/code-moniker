-- Phase 3: TypeScript extraction. End-to-end: source text in,
-- code_graph out with the right defs and refs.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(20);

-- Surface presence ----------------------------------------------------------

SELECT has_function('extract_typescript'::name,
	ARRAY['text','text','moniker','boolean','text[]'],
	'extract_typescript(text, text, moniker, boolean, text[]) is exposed');

-- Extracting an empty source still produces a graph rooted at the module.

WITH empty AS (
	SELECT extract_typescript(
		'util.ts',
		'',
		'esac+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is(graph_root(g)::text, 'esac+moniker://app/path:main/path:util',
		'module moniker = anchor + file basename (extension stripped)') AS r1,
	is(array_length(graph_def_monikers(g), 1), 1,
		'empty source yields a graph with the module def only') AS r2
FROM empty;

-- Class with a method emits the class and the method as defs.

WITH g AS (
	SELECT extract_typescript(
		'src/Foo.ts',
		'export class Foo { bar(a, b) { return a; } }',
		'esac+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/path:main/path:src/path:Foo'::moniker,
		'graph contains the module moniker') AS r3,
	ok(g @> 'esac+moniker://app/path:main/path:src/path:Foo/class:Foo'::moniker,
		'graph contains the class def') AS r4,
	ok(g @> 'esac+moniker://app/path:main/path:src/path:Foo/class:Foo/method:bar(2)'::moniker,
		'method moniker carries arity in segment name') AS r5
FROM g;

-- Imports decompose into one ref per named specifier; bare specifiers
-- become external_pkg targets.

WITH g AS (
	SELECT extract_typescript(
		'src/index.ts',
		'import { foo, bar } from "./util";',
		'esac+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is(array_length(graph_ref_targets(g), 1), 2,
		'two named specifiers produce two refs') AS r6,
	ok('esac+moniker://app/path:main/path:src/path:util/path:foo'::moniker = ANY(graph_ref_targets(g)),
		'imports_symbol target = resolved-module + path:<name>') AS r7
FROM g;

WITH g AS (
	SELECT extract_typescript(
		'src/index.ts',
		'import { useState } from "react";',
		'esac+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok('esac+moniker://app/external_pkg:react/path:useState'::moniker = ANY(graph_ref_targets(g)),
		'bare specifier resolves under project + external_pkg') AS r8
FROM g;

-- Class heritage and decorators produce refs.

WITH g AS (
	SELECT extract_typescript(
		'src/Foo.ts',
		'@Decor class Foo extends Base implements I {}',
		'esac+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok('esac+moniker://app/path:main/path:src/path:Foo/class:Base'::moniker = ANY(graph_ref_targets(g)),
		'extends emits a class:<name> target') AS r9,
	ok('esac+moniker://app/path:main/path:src/path:Foo/interface:I'::moniker = ANY(graph_ref_targets(g)),
		'implements emits an interface:<name> target') AS r10,
	ok('esac+moniker://app/path:main/path:src/path:Foo/function:Decor()'::moniker = ANY(graph_ref_targets(g)),
		'decorator emits a function-shaped annotates target') AS r11
FROM g;

-- Deep extraction surfaces params and locals.

WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'function f(a, b) { let sum = a + b; }',
		'esac+moniker://app/path:main'::moniker,
		deep := true
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/path:main/path:util/function:f(2)/param:a'::moniker,
		'deep=true surfaces parameter defs') AS r12,
	ok(g @> 'esac+moniker://app/path:main/path:util/function:f(2)/local:sum'::moniker,
		'deep=true surfaces local defs') AS r13
FROM g;

-- Method call receiver hint surfaces in graph_refs.receiver_hint.

WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'class C { m() { this.bar(); } }',
		'esac+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is((SELECT receiver_hint FROM graph_refs(g) WHERE kind = 'method_call'),
		'this',
		'this.bar() carries receiver_hint=this') AS r14
FROM g;

-- DI register stays silent without a preset.

WITH no_preset AS (
	SELECT extract_typescript(
		'util.ts',
		'register(UserService);',
		'esac+moniker://app/path:main'::moniker
	) AS g
), with_preset AS (
	SELECT extract_typescript(
		'util.ts',
		'register(UserService);',
		'esac+moniker://app/path:main'::moniker,
		false,
		ARRAY['register']::text[]
	) AS g
)
SELECT
	is((SELECT count(*)::int FROM no_preset, graph_refs(g) WHERE kind = 'di_register'),
		0,
		'di_register silent without preset') AS r15,
	is((SELECT count(*)::int FROM with_preset, graph_refs(g) WHERE kind = 'di_register'),
		1,
		'di_register fires when callee is in preset list') AS r16;

-- Visibility surfaces in graph_defs.

WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'export class A {} class B {}',
		'esac+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is((SELECT visibility FROM graph_defs(g) WHERE kind = 'class'
	     AND moniker = 'esac+moniker://app/path:main/path:util/class:A'::moniker),
		'public',
		'exported class is public') AS r17,
	is((SELECT visibility FROM graph_defs(g) WHERE kind = 'class'
	     AND moniker = 'esac+moniker://app/path:main/path:util/class:B'::moniker),
		'module',
		'unexported class is module-visible') AS r18
FROM g;

-- Alias surfaces on aliased imports.

WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'import { X as Y } from "./foo";',
		'esac+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is((SELECT alias FROM graph_refs(g) WHERE kind = 'imports_symbol'),
		'Y',
		'import { X as Y } records alias=Y') AS r19
FROM g;

-- Confidence distinguishes relative vs external imports.

WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'import a from "./local"; import b from "react";',
		'esac+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is((SELECT array_agg(DISTINCT confidence ORDER BY confidence)
	    FROM graph_refs(g) WHERE kind = 'imports_symbol'),
		ARRAY['external','imported']::text[],
		'imports_symbol gets imported/external confidence based on specifier') AS r20
FROM g;

SELECT * FROM finish();

ROLLBACK;
