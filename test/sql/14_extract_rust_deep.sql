
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(9);

SELECT has_function('extract_rust'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_rust(4-arg) is exposed');


WITH g AS (
	SELECT extract_rust(
		'util.rs',
		'pub fn add(a: i32, b: i32) -> i32 { let sum = a + b; sum }',
		'esac+moniker://pkg'::moniker
	) AS g
)
SELECT
	is(array_length(graph_def_monikers(g), 1), 2,
		'shallow extract: only module + add() (no param/local)') AS r1,
	ok(NOT (g @> 'esac+moniker://pkg/lang:rs/module:util/function:add(i32,i32)/param:a'::moniker),
		'shallow extract: no param defs') AS r2,
	ok(NOT (g @> 'esac+moniker://pkg/lang:rs/module:util/function:add(i32,i32)/local:sum'::moniker),
		'shallow extract: no local defs') AS r3
FROM g;


WITH g AS (
	SELECT extract_rust(
		'util.rs',
		'pub fn add(a: i32, b: i32) -> i32 { let sum = a + b; sum }',
		'esac+moniker://pkg'::moniker,
		deep := true
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://pkg/lang:rs/module:util/function:add(i32,i32)/param:a'::moniker,
		'deep extract emits param:a under function:add(i32,i32)') AS r4,
	ok(g @> 'esac+moniker://pkg/lang:rs/module:util/function:add(i32,i32)/param:b'::moniker,
		'deep extract emits param:b') AS r5,
	ok(g @> 'esac+moniker://pkg/lang:rs/module:util/function:add(i32,i32)/local:sum'::moniker,
		'deep extract emits local:sum from let-binding') AS r6
FROM g;


WITH g AS (
	SELECT extract_rust(
		'util.rs',
		'pub struct Foo; impl Foo { fn bar(&self, x: i32) {} }',
		'esac+moniker://pkg'::moniker,
		deep := true
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://pkg/lang:rs/module:util/class:Foo/method:bar(i32)/param:self'::moniker,
		'deep extract emits param:self for &self (self implicit in moniker)') AS r7
FROM g;


WITH g AS (
	SELECT extract_rust(
		'util.rs',
		'pub fn run() { let f = |x| x + 1; }',
		'esac+moniker://pkg'::moniker,
		deep := true
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://pkg/lang:rs/module:util/function:run()/function:f(_)'::moniker,
		'deep extract emits named closure with `_` placeholder for untyped param') AS r8
FROM g;

SELECT * FROM finish();

ROLLBACK;
