-- Java extraction smoke test.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(8);

SELECT has_function('extract_java'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_java(text, text, moniker, boolean) is exposed');

-- Module moniker = anchor + package: segments + module:basename.

WITH g AS (
	SELECT extract_java(
		'src/main/java/com/acme/Foo.java',
		E'package com.acme;\npublic class Foo {}\n',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	is(graph_root(g)::text,
		'esac+moniker://app/package:com/package:acme/module:Foo',
		'package decl drives the module moniker') AS r1,
	ok(g @> 'esac+moniker://app/package:com/package:acme/module:Foo/class:Foo'::moniker,
		'class def lives under the module') AS r2
FROM g;

-- Method arity in segment + signature column populated.

WITH g AS (
	SELECT extract_java(
		'Foo.java',
		'public class Foo { public int bar(int a, String b) { return a; } }',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/module:Foo/class:Foo/method:bar(2)'::moniker,
		'method moniker carries arity') AS r3,
	is((SELECT signature FROM graph_defs(g) WHERE kind = 'method'),
		'int,String',
		'method signature column lists parameter types') AS r4
FROM g;

-- Visibility default is `package` (not `public` like TS).

WITH g AS (
	SELECT extract_java(
		'Foo.java',
		'class Foo {}',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT visibility FROM graph_defs(g) WHERE kind = 'class'),
		'package',
		'unmodified Java class is package-visible') AS r5
FROM g;

-- JDK imports get confidence=external; project imports get imported.

WITH g AS (
	SELECT extract_java(
		'Foo.java',
		E'import java.util.List;\nimport com.acme.Helpers;\nclass Foo {}\n',
		'esac+moniker://app'::moniker
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
		'com.acme.Helpers import marked imported') AS r7;

-- this.bar() carries receiver_hint=this.

WITH g AS (
	SELECT extract_java(
		'Foo.java',
		'class Foo { void m() { this.bar(); } void bar() {} }',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT receiver_hint FROM graph_refs(g) WHERE kind = 'method_call'),
		'this',
		'method_call receiver_hint=this') AS r8
FROM g;

SELECT * FROM finish();

ROLLBACK;
