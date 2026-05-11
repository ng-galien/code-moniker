BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(6);

WITH g AS (
	SELECT extract_typescript(
		'src/util.ts',
		'// a' || E'\n' || '/* b */' || E'\n' || 'class Foo {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	2,
	'TS extract_typescript emits one comment def per AST comment node');

WITH g AS (
	SELECT extract_rust(
		'src/lib.rs',
		'// a' || E'\n' || '/// b' || E'\n' || 'struct Foo;',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	2,
	'Rust extract_rust emits one comment def per AST comment node');

WITH g AS (
	SELECT extract_java(
		'src/Foo.java',
		'// a' || E'\n' || '/* b */' || E'\n' || 'class Foo {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	2,
	'Java extract_java emits one comment def per AST comment node');

WITH g AS (
	SELECT extract_python(
		'acme/foo.py',
		'# a' || E'\n' || '# b' || E'\n' || 'class Foo: pass',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	2,
	'Python extract_python emits one comment def per AST comment node');

WITH g AS (
	SELECT extract_go(
		'foo.go',
		'package foo' || E'\n' || '// a' || E'\n' || '/* b */' || E'\n' || 'func Bar() {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	2,
	'Go extract_go emits one comment def per AST comment node');

WITH g AS (
	SELECT extract_csharp(
		'Foo.cs',
		'// a' || E'\n' || '/// b' || E'\n' || 'class Foo {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	2,
	'C# extract_csharp emits one comment def per AST comment node');

SELECT * FROM finish();

ROLLBACK;
