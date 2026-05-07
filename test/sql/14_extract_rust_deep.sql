-- Deep extraction for Rust: params, locals, named closures emitted
-- when `deep := true`. Off by default (the call-site shape that
-- existing pgTAP tests rely on must keep working) — opt-in via the
-- 4th argument.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(9);

SELECT has_function('extract_rust'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_rust(4-arg) is exposed');

-- Default deep=false reproduces shallow R-1 behavior --------------------

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
	ok(NOT (g @> 'esac+moniker://pkg/module:util/function:add(2)/param:a'::moniker),
		'shallow extract: no param defs') AS r2,
	ok(NOT (g @> 'esac+moniker://pkg/module:util/function:add(2)/local:sum'::moniker),
		'shallow extract: no local defs') AS r3
FROM g;

-- deep=true emits params and locals under the function ------------------

WITH g AS (
	SELECT extract_rust(
		'util.rs',
		'pub fn add(a: i32, b: i32) -> i32 { let sum = a + b; sum }',
		'esac+moniker://pkg'::moniker,
		deep := true
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://pkg/module:util/function:add(2)/param:a'::moniker,
		'deep extract emits param:a under function:add(2)') AS r4,
	ok(g @> 'esac+moniker://pkg/module:util/function:add(2)/param:b'::moniker,
		'deep extract emits param:b') AS r5,
	ok(g @> 'esac+moniker://pkg/module:util/function:add(2)/local:sum'::moniker,
		'deep extract emits local:sum from let-binding') AS r6
FROM g;

-- self parameter on impl methods collapses to param:self ----------------

WITH g AS (
	SELECT extract_rust(
		'util.rs',
		'pub struct Foo; impl Foo { fn bar(&self, x: i32) {} }',
		'esac+moniker://pkg'::moniker,
		deep := true
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://pkg/module:util/class:Foo/method:bar(2)/param:self'::moniker,
		'deep extract emits param:self for &self') AS r7
FROM g;

-- Named closure (let f = |x| ...) emits a function def under the parent --

WITH g AS (
	SELECT extract_rust(
		'util.rs',
		'pub fn run() { let f = |x| x + 1; }',
		'esac+moniker://pkg'::moniker,
		deep := true
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://pkg/module:util/function:run()/function:f(1)'::moniker,
		'deep extract emits named closure as function def under run()') AS r8
FROM g;

SELECT * FROM finish();

ROLLBACK;
