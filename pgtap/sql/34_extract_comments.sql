BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(6);

WITH g AS (
	SELECT extract_typescript(
		'src/util.ts',
		'// a' || E'\n' || '// b' || E'\n' || '// c' || E'\n' || 'class Foo {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	1,
	'TS extract_typescript collapses three adjacent line comments into one comment def');

WITH g AS (
	SELECT extract_rust(
		'src/lib.rs',
		'// a' || E'\n' || '// b' || E'\n' || '// c' || E'\n' || 'struct Foo;',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	1,
	'Rust extract_rust collapses three adjacent line comments into one comment def');

WITH g AS (
	SELECT extract_java(
		'src/Foo.java',
		'// a' || E'\n' || '// b' || E'\n' || '// c' || E'\n' || 'class Foo {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	1,
	'Java extract_java collapses three adjacent line comments into one comment def');

WITH g AS (
	SELECT extract_python(
		'acme/foo.py',
		'# a' || E'\n' || '# b' || E'\n' || '# c' || E'\n' || 'class Foo: pass',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	1,
	'Python extract_python collapses three adjacent line comments into one comment def');

WITH g AS (
	SELECT extract_go(
		'foo.go',
		'package foo' || E'\n' || '// a' || E'\n' || '// b' || E'\n' || '// c' || E'\n' || 'func Bar() {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	1,
	'Go extract_go collapses three adjacent line comments into one comment def');

WITH g AS (
	SELECT extract_csharp(
		'Foo.cs',
		'// a' || E'\n' || '// b' || E'\n' || '// c' || E'\n' || 'class Foo {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT count(*)::int FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	1,
	'C# extract_csharp collapses three adjacent line comments into one comment def');

SELECT * FROM finish();

ROLLBACK;
