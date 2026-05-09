
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(13);

SELECT has_function('extract_csharp'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_csharp(text, text, moniker, boolean) is exposed');


WITH g AS (
	SELECT extract_csharp(
		'Acme/Util/Text.cs',
		E'namespace Acme.Util;\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT is(graph_root(g)::text,
	'pcm+moniker://app/lang:cs/package:Acme/package:Util/module:Text',
	'file path drives the module moniker (path-based)')
FROM g;


WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'namespace Foo;\npublic class Bar {\n    public int Add(int a, int b) { return a + b; }\n}\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:cs/module:F/class:Bar/method:Add(int,int)'::moniker,
		'method moniker carries full parameter type signature') AS r1,
	is((SELECT signature FROM graph_defs(g) WHERE kind = 'method'),
		'int,int',
		'method signature column lists parameter types') AS r2
FROM g;


WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'namespace Foo;\npublic record Person(int Age, string Name);\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:cs/module:F/record:Person'::moniker,
		'record emits record def') AS r3,
	ok(g @> 'pcm+moniker://app/lang:cs/module:F/record:Person/constructor:Person(int,string)'::moniker,
		'record primary constructor synthesised under the record') AS r4
FROM g;


WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'namespace Foo;\nclass Bar {}\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT is((SELECT visibility FROM graph_defs(g) WHERE kind = 'class'),
	'package',
	'top-level type without modifier defaults to internal (=VIS_PACKAGE)')
FROM g;


WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'using System;\nusing Newtonsoft.Json;\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT confidence FROM graph_refs(g) r
	     WHERE r.kind = 'imports_module' AND r.target::text LIKE '%System%'),
		'external',
		'System namespace marked external') AS r5,
	is((SELECT confidence FROM graph_refs(g) r
	     WHERE r.kind = 'imports_module' AND r.target::text LIKE '%Newtonsoft%'),
		'imported',
		'third-party namespace marked imported') AS r6
FROM g;


WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'namespace Foo;\npublic class Base {}\npublic class Bar : Base {}\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT count(*)::int FROM graph_refs(g) WHERE kind = 'extends'),
		1,
		'base list emits one EXTENDS edge') AS r7
FROM g;


WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'namespace Foo;\n[Serializable]\npublic class Bar {}\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT is((SELECT count(*)::int FROM graph_refs(g) WHERE kind = 'annotates'),
	1,
	'class attribute emits annotates ref')
FROM g;


WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'class B {\n    void M() { Console.WriteLine("hi"); }\n}\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT is((SELECT receiver_hint FROM graph_refs(g) WHERE kind = 'method_call'),
	'Console',
	'member access call carries identifier as receiver_hint')
FROM g;


WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'class B {\n    void M(int a) { var x = 1; }\n}\n',
		'pcm+moniker://app'::moniker,
		true
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:cs/module:F/class:B/method:M(int)/param:a'::moniker,
		'deep extraction emits param def') AS r8
FROM g;


SELECT * FROM finish();

ROLLBACK;
