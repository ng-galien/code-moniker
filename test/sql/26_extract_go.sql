
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(11);

SELECT has_function('extract_go'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_go(text, text, moniker, boolean) is exposed');


WITH g AS (
	SELECT extract_go(
		'acme/util/text.go',
		E'package text\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT is(graph_root(g)::text,
	'pcm+moniker://app/lang:go/package:acme/package:util/module:text',
	'file path drives the module moniker (path-based, not package clause)')
FROM g;


WITH g AS (
	SELECT extract_go(
		'm.go',
		E'package foo\nfunc Add(a int, b int) int { return a + b }\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:go/module:m/function:Add(int,int)'::moniker,
		'function moniker carries full parameter type signature') AS r1,
	is((SELECT signature FROM graph_defs(g) WHERE kind = 'function'),
		'int,int',
		'function signature column lists parameter types') AS r2
FROM g;


WITH g AS (
	SELECT extract_go(
		'm.go',
		E'package foo\ntype Foo struct{}\nfunc (r *Foo) Bar(x int) {}\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/lang:go/module:m/class:Foo/method:Bar(int)'::moniker,
		'method moniker reparented under receiver type, pointer star stripped') AS r3
FROM g;


WITH g AS (
	SELECT extract_go(
		'm.go',
		E'package foo\nfunc Hello() {}\nfunc helper() {}\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT visibility FROM graph_defs(g) d
	     WHERE kind = 'function' AND
	           moniker = 'pcm+moniker://app/lang:go/module:m/function:Hello()'::moniker),
		'public',
		'capitalized name is public') AS r4,
	is((SELECT visibility FROM graph_defs(g) d
	     WHERE kind = 'function' AND
	           moniker = 'pcm+moniker://app/lang:go/module:m/function:helper()'::moniker),
		'module',
		'lowercase name is module-private') AS r5
FROM g;


WITH g AS (
	SELECT extract_go(
		'm.go',
		E'package foo\nimport (\n\t"fmt"\n\t"github.com/x/y"\n)\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT confidence FROM graph_refs(g) r
	     WHERE r.kind = 'imports_module' AND r.target::text LIKE '%fmt%'),
		'external',
		'stdlib import marked external') AS r6,
	is((SELECT confidence FROM graph_refs(g) r
	     WHERE r.kind = 'imports_module' AND r.target::text LIKE '%github.com%'),
		'imported',
		'third-party import marked imported') AS r7
FROM g;


WITH g AS (
	SELECT extract_go(
		'm.go',
		E'package foo\nimport "net/http"\nfunc Run() { http.Get("u") }\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://app/external_pkg:net/path:http/function:Get(1)'::moniker IS NOT NULL,
		'package-qualified call target preserves full import path') AS r8
FROM g;


WITH g AS (
	SELECT extract_go(
		'm.go',
		E'package foo\ntype Base struct{}\ntype Derived struct { Base; X int }\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT count(*)::int FROM graph_refs(g) WHERE kind = 'extends'),
		1,
		'struct embedding emits one EXTENDS edge per embedded type') AS r9
FROM g;


SELECT * FROM finish();

ROLLBACK;
