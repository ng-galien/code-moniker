
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(11);


WITH g AS (
	SELECT extract_typescript(
		'src/Foo.ts',
		'export class Foo { bar() {} }',
		'pcm+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok((SELECT start_byte IS NOT NULL AND end_byte IS NOT NULL
	    FROM graph_defs(g) WHERE kind = 'class'),
		'TS class def has both start_byte and end_byte') AS r1,
	ok((SELECT start_byte < end_byte
	    FROM graph_defs(g) WHERE kind = 'class'),
		'TS class def: start_byte < end_byte') AS r2,
	ok((SELECT start_byte IS NOT NULL AND end_byte IS NOT NULL
	    FROM graph_refs(g) WHERE kind = 'reexports' OR kind = 'imports_symbol'
	    LIMIT 1) IS NOT FALSE,
		'TS refs (when present) carry byte positions') AS r3
FROM g;


WITH g AS (
	SELECT extract_rust(
		'src/lib.rs',
		'pub fn answer() -> i32 { 42 }',
		'pcm+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok((SELECT start_byte IS NOT NULL AND end_byte IS NOT NULL
	    FROM graph_defs(g) WHERE kind = 'function'),
		'Rust function def has byte range') AS r4,
	ok((SELECT start_byte < end_byte
	    FROM graph_defs(g) WHERE kind = 'function'),
		'Rust function def: start_byte < end_byte') AS r5
FROM g;


WITH g AS (
	SELECT extract_java(
		'src/Foo.java',
		'package x; public class Foo { void bar() {} }',
		'pcm+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok((SELECT start_byte IS NOT NULL AND end_byte IS NOT NULL
	    FROM graph_defs(g) WHERE kind = 'class'),
		'Java class def has byte range') AS r6,
	ok((SELECT start_byte < end_byte
	    FROM graph_defs(g) WHERE kind = 'class'),
		'Java class def: start_byte < end_byte') AS r7
FROM g;


WITH g AS (
	SELECT extract_python(
		'mod.py',
		E'def f(x):\n    return x\n',
		'pcm+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok((SELECT start_byte IS NOT NULL AND end_byte IS NOT NULL
	    FROM graph_defs(g) WHERE kind = 'function'),
		'Python function def has byte range') AS r8,
	ok((SELECT start_byte < end_byte
	    FROM graph_defs(g) WHERE kind = 'function'),
		'Python function def: start_byte < end_byte') AS r9
FROM g;


WITH g AS (
	SELECT extract_plpgsql(
		'pkg.sql',
		'CREATE FUNCTION f() RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;',
		'pcm+moniker://app/path:main'::moniker
	) AS g
)
SELECT
	ok((SELECT start_byte IS NOT NULL AND end_byte IS NOT NULL
	    FROM graph_defs(g) WHERE kind = 'function'),
		'SQL function def has byte range') AS r10,
	ok((SELECT start_byte <= end_byte
	    FROM graph_defs(g) WHERE kind = 'function'),
		'SQL function def: start_byte <= end_byte') AS r11
FROM g;

SELECT * FROM finish();

ROLLBACK;
