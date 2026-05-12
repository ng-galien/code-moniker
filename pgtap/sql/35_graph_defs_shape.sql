BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(8);

WITH g AS (
	SELECT extract_typescript(
		'src/util.ts',
		'class Foo { bar() { let x = 1; } }',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT shape FROM g, graph_defs(g.graph) d WHERE d.kind = 'class'),
	'type',
	'class def projects shape=type');

WITH g AS (
	SELECT extract_typescript(
		'src/util.ts',
		'class Foo { bar() { let x = 1; } }',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT opens_scope FROM g, graph_defs(g.graph) d WHERE d.kind = 'class'),
	true,
	'class def opens scope');

WITH g AS (
	SELECT extract_typescript(
		'src/util.ts',
		'class Foo { bar() { let x = 1; } }',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT shape FROM g, graph_defs(g.graph) d WHERE d.kind = 'method'),
	'callable',
	'method def projects shape=callable');

WITH g AS (
	SELECT extract_typescript(
		'src/util.ts',
		'// hello' || E'\n' || 'class Foo {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT shape FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	'annotation',
	'comment def projects shape=annotation');

WITH g AS (
	SELECT extract_typescript(
		'src/util.ts',
		'// hello' || E'\n' || 'class Foo {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT opens_scope FROM g, graph_defs(g.graph) d WHERE d.kind = 'comment'),
	false,
	'comment def does not open a scope');

WITH g AS (
	SELECT extract_typescript(
		'src/util.ts',
		'class Foo {}',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT shape FROM g, graph_defs(g.graph) d WHERE d.kind = 'module'),
	'namespace',
	'module def projects shape=namespace');

WITH g AS (
	SELECT extract_rust(
		'src/lib.rs',
		'pub fn add(a: i32, b: i32) -> i32 { a + b }',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT shape FROM g, graph_defs(g.graph) d WHERE d.kind = 'fn'),
	'callable',
	'Rust fn def projects shape=callable across languages');

WITH g AS (
	SELECT extract_rust(
		'src/lib.rs',
		'pub struct Foo;',
		'code+moniker://app'::moniker
	) AS graph
)
SELECT is(
	(SELECT shape FROM g, graph_defs(g.graph) d WHERE d.kind = 'struct'),
	'type',
	'Rust struct def projects shape=type across languages');

SELECT * FROM finish();

ROLLBACK;
