
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(24);


SELECT has_function('extract_typescript'::name,
	ARRAY['text','text','moniker','boolean','text[]'],
	'extract_typescript(text, text, moniker, boolean, text[]) is exposed');


WITH empty AS (
	SELECT extract_typescript(
		'util.ts',
		'',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is(graph_root(g)::text, 'code+moniker://app/path:main/lang:ts/module:util',
		'module moniker = anchor + file basename (extension stripped)') AS r1,
	is(array_length(graph_def_monikers(g), 1), 1,
		'empty source yields a graph with the module def only') AS r2
FROM empty;


WITH g AS (
	SELECT extract_typescript(
		'src/Foo.ts',
		'export class Foo { bar(a, b) { return a; } }',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok(g @> 'code+moniker://app/path:main/lang:ts/dir:src/module:Foo'::moniker,
		'graph contains the module moniker') AS r3,
	ok(g @> 'code+moniker://app/path:main/lang:ts/dir:src/module:Foo/class:Foo'::moniker,
		'graph contains the class def') AS r4,
	ok(g @> 'code+moniker://app/path:main/lang:ts/dir:src/module:Foo/class:Foo/method:bar(_,_)'::moniker,
		'method moniker carries `_` placeholders for unannotated JS params') AS r5
FROM g;


WITH g AS (
	SELECT extract_typescript(
		'src/index.ts',
		'import { foo, bar } from "./util";',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is(array_length(graph_ref_targets(g), 1), 2,
		'two named specifiers produce two refs') AS r6,
	ok('code+moniker://app/path:main/lang:ts/dir:src/module:util/path:foo'::moniker = ANY(graph_ref_targets(g)),
		'imports_symbol target = resolved-module + path:<name>') AS r7
FROM g;

WITH g AS (
	SELECT extract_typescript(
		'src/index.ts',
		'import { useState } from "react";',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok('code+moniker://app/external_pkg:react/path:useState'::moniker = ANY(graph_ref_targets(g)),
		'bare specifier resolves under project + external_pkg') AS r8
FROM g;


WITH g AS (
	SELECT extract_typescript(
		'src/Foo.ts',
		'@Decor class Foo extends Base implements I {}',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok('code+moniker://app/path:main/lang:ts/dir:src/module:Foo/class:Base'::moniker = ANY(graph_ref_targets(g)),
		'extends emits a class:<name> target') AS r9,
	ok('code+moniker://app/path:main/lang:ts/dir:src/module:Foo/interface:I'::moniker = ANY(graph_ref_targets(g)),
		'implements emits an interface:<name> target') AS r10,
	ok('code+moniker://app/path:main/lang:ts/dir:src/module:Foo/function:Decor()'::moniker = ANY(graph_ref_targets(g)),
		'decorator emits a function-shaped annotates target') AS r11
FROM g;


WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'function f(a, b) { let sum = a + b; }',
		'code+moniker://app/path:main'::moniker,
		deep := true
	) AS g
)
SELECT
	ok(g @> 'code+moniker://app/path:main/lang:ts/module:util/function:f(_,_)/param:a'::moniker,
		'deep=true surfaces parameter defs') AS r12,
	ok(g @> 'code+moniker://app/path:main/lang:ts/module:util/function:f(_,_)/local:sum'::moniker,
		'deep=true surfaces local defs') AS r13
FROM g;


WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'class C { m() { this.bar(); } }',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is((SELECT receiver_hint FROM graph_refs(g) WHERE kind = 'method_call'),
		'this',
		'this.bar() carries receiver_hint=this') AS r14
FROM g;


WITH no_preset AS (
	SELECT extract_typescript(
		'util.ts',
		'register(UserService);',
		'code+moniker://app/path:main'::moniker
	) AS g
), with_preset AS (
	SELECT extract_typescript(
		'util.ts',
		'register(UserService);',
		'code+moniker://app/path:main'::moniker,
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


WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'export class A {} class B {}',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is((SELECT visibility FROM graph_defs(g) WHERE kind = 'class'
	     AND moniker = 'code+moniker://app/path:main/lang:ts/module:util/class:A'::moniker),
		'public',
		'exported class is public') AS r17,
	is((SELECT visibility FROM graph_defs(g) WHERE kind = 'class'
	     AND moniker = 'code+moniker://app/path:main/lang:ts/module:util/class:B'::moniker),
		'module',
		'unexported class is module-visible') AS r18
FROM g;


WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'import { X as Y } from "./foo";',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is((SELECT alias FROM graph_refs(g) WHERE kind = 'imports_symbol'),
		'Y',
		'import { X as Y } records alias=Y') AS r19
FROM g;


WITH g AS (
	SELECT extract_typescript(
		'util.ts',
		'import a from "./local"; import b from "react";',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is((SELECT array_agg(DISTINCT confidence ORDER BY confidence)
	    FROM graph_refs(g) WHERE kind = 'imports_symbol'),
		ARRAY['external','imported']::text[],
		'imports_symbol gets imported/external confidence based on specifier') AS r20
FROM g;


WITH g AS (
	SELECT extract_typescript(
		'explorer.ts',
		'import { z } from ''zod''; const s = z.string();',
		'code+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	is((SELECT receiver_hint FROM graph_refs(g) WHERE kind = 'method_call'),
		'z',
		'identifier-shaped receiver carries the alias text, not the constant tag') AS r21
FROM g;


WITH member_callee AS (
	SELECT extract_typescript(
		'util.ts',
		'container.register(''repo'', makeRepo);',
		'code+moniker://app/path:main'::moniker,
		false,
		ARRAY['register']::text[]
	) AS g
), wrapped_factory AS (
	SELECT extract_typescript(
		'util.ts',
		'register(''repo'', asFunction(makeRepo).singleton());',
		'code+moniker://app/path:main'::moniker,
		false,
		ARRAY['register']::text[]
	) AS g
)
SELECT
	cmp_ok((SELECT count(*)::int FROM member_callee, graph_refs(g) WHERE kind = 'di_register'),
		'>', 0,
		'container.register(name, factory) emits di_register via member-expression callee') AS r22,
	cmp_ok((SELECT count(*)::int FROM wrapped_factory, graph_refs(g) WHERE kind = 'di_register'),
		'>', 0,
		'register(name, asFunction(make).singleton()) recurses through wrapper + chain to find make') AS r23;

SELECT * FROM finish();

ROLLBACK;
