
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(9);

SELECT has_function('extract_java'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_java(text, text, moniker, boolean) is exposed');


WITH g AS (
	SELECT extract_java(
		'src/main/java/com/acme/Foo.java',
		E'package com.acme;\npublic class Foo {}\n',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	is(graph_root(g)::text,
		'code+moniker://app/lang:java/package:com/package:acme/module:Foo',
		'package decl drives the module moniker') AS r1,
	ok(g @> 'code+moniker://app/lang:java/package:com/package:acme/module:Foo/class:Foo'::moniker,
		'class def lives under the module') AS r2
FROM g;


WITH g AS (
	SELECT extract_java(
		'Foo.java',
		'public class Foo { public int bar(int a, String b) { return a; } }',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'code+moniker://app/lang:java/module:Foo/class:Foo/method:bar(a:int,b:String)'::moniker,
		'method moniker carries name:type slot signature') AS r3,
	is((SELECT signature FROM graph_defs(g) WHERE kind = 'method'),
		'a:int,b:String',
		'method signature column lists name:type slots') AS r4
FROM g;


WITH g AS (
	SELECT extract_java(
		'Foo.java',
		'class Foo {}',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT visibility FROM graph_defs(g) WHERE kind = 'class'),
		'package',
		'unmodified Java class is package-visible') AS r5
FROM g;


WITH g AS (
	SELECT extract_java(
		'Foo.java',
		E'import java.util.List;\nimport com.acme.Helpers;\nclass Foo {}\n',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT confidence FROM graph_refs(g)
	     WHERE kind = 'imports_symbol' AND target::text LIKE '%List%'),
		'external',
		'java.util.List import marked external') AS r6,
	is((SELECT confidence FROM graph_refs(g)
	     WHERE kind = 'imports_symbol' AND target::text LIKE '%Helpers%'),
		'imported',
		'com.acme.Helpers import marked imported') AS r7
FROM g;


WITH g AS (
	SELECT extract_java(
		'Foo.java',
		'class Foo { void m() { this.bar(); } void bar() {} }',
		'code+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT receiver_hint FROM graph_refs(g) WHERE kind = 'method_call'),
		'this',
		'method_call receiver_hint=this') AS r8
FROM g;

SELECT * FROM finish();

ROLLBACK;
