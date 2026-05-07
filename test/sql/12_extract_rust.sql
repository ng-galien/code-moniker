-- Rust extractor: end-to-end SQL surface. Source text in,
-- code_graph out with the right defs and refs.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(9);

SELECT has_function('extract_rust'::name,
	ARRAY['text','text','moniker'],
	'extract_rust(text, text, moniker) is exposed');

-- Module root from URI: dir segments as `path:`, basename as `module:`.
WITH g AS (
	SELECT extract_rust(
		'src/core/moniker/view.rs',
		'',
		'esac+moniker://pg_code_moniker'::moniker
	) AS g
)
SELECT is(
	graph_root(g)::text,
	'esac+moniker://pg_code_moniker/path:src/path:core/path:moniker/module:view',
	'module moniker = anchor + path:dir + module:basename')
FROM g;

-- Struct + impl block: methods land under class:Foo, not under the
-- impl block itself.
WITH g AS (
	SELECT extract_rust(
		'util.rs',
		$rs$
pub struct Foo;
impl Foo {
    pub fn bar(&self) -> i32 { 0 }
    pub fn baz(&self, n: u32) {}
}
$rs$,
		'esac+moniker://pkg'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://pkg/module:util/class:Foo'::moniker,
		'struct emits class def') AS r1,
	ok(g @> 'esac+moniker://pkg/module:util/class:Foo/method:bar(1)'::moniker,
		'impl method re-parented onto struct, arity counts &self') AS r2,
	ok(g @> 'esac+moniker://pkg/module:util/class:Foo/method:baz(2)'::moniker,
		'second impl method with &self + n') AS r3
FROM g;

-- impl Trait for Type → implements ref from Type → Trait.
WITH g AS (
	SELECT extract_rust(
		'util.rs',
		$rs$
pub trait Greet { fn hi(&self); }
pub struct Foo;
impl Greet for Foo { fn hi(&self) {} }
$rs$,
		'esac+moniker://pkg'::moniker
	) AS g
)
SELECT
	ok(EXISTS (
		SELECT 1 FROM graph_refs(g) r
		 WHERE r.kind = 'implements'
		   AND r.source = 'esac+moniker://pkg/module:util/class:Foo'::moniker
		   AND r.target = 'esac+moniker://pkg/module:util/interface:Greet'::moniker),
		'impl Trait for Type emits implements ref Type → Trait') AS r4
FROM g;

-- use std::collections::{HashMap, HashSet}; → two imports_symbol refs.
WITH g AS (
	SELECT extract_rust(
		'util.rs',
		'use std::collections::{HashMap, HashSet};',
		'esac+moniker://pkg'::moniker
	) AS g
)
SELECT
	is(array_length(graph_ref_targets(g), 1), 2,
		'group import emits one ref per leaf') AS r5,
	ok('esac+moniker://pkg/path:std/path:collections/path:HashMap'::moniker
	     = ANY(graph_ref_targets(g)),
		'first leaf reaches HashMap') AS r6,
	ok('esac+moniker://pkg/path:std/path:collections/path:HashSet'::moniker
	     = ANY(graph_ref_targets(g)),
		'second leaf reaches HashSet') AS r7
FROM g;

SELECT * FROM finish();

ROLLBACK;
