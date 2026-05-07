-- Rust extractor: end-to-end SQL surface. Source text in,
-- code_graph out with the right defs and refs.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(11);

SELECT has_function('extract_rust'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_rust(text, text, moniker, boolean) is exposed');

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
	ok(g @> 'esac+moniker://pkg/module:util/class:Foo/method:bar()'::moniker,
		'impl method re-parented onto struct; &self is implicit so arity 0 with empty parens') AS r2,
	ok(g @> 'esac+moniker://pkg/module:util/class:Foo/method:baz(u32)'::moniker,
		'second impl method with one value parameter (self excluded)') AS r3
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
	ok('esac+moniker://pkg/external_pkg:std/path:collections/path:HashMap'::moniker
	     = ANY(graph_ref_targets(g)),
		'first leaf reaches HashMap under external_pkg:std') AS r6,
	ok('esac+moniker://pkg/external_pkg:std/path:collections/path:HashSet'::moniker
	     = ANY(graph_ref_targets(g)),
		'second leaf reaches HashSet under external_pkg:std') AS r7
FROM g;

-- Mixed project-local + external in one source: `crate::` resolves
-- under the project anchor, bare crate names land under external_pkg.
WITH g AS (
	SELECT extract_rust(
		'util.rs',
		$rs$
use crate::core::moniker::Moniker;
use pgrx::prelude::*;
$rs$,
		'esac+moniker://pkg'::moniker
	) AS g
)
SELECT
	ok('esac+moniker://pkg/path:core/path:moniker/path:Moniker'::moniker
	     = ANY(graph_ref_targets(g)),
		'crate:: prefix resolves under the project anchor (no external_pkg)') AS r8,
	ok('esac+moniker://pkg/external_pkg:pgrx/path:prelude'::moniker
	     = ANY(graph_ref_targets(g)),
		'bare external crate root marked with external_pkg:') AS r9
FROM g;

SELECT * FROM finish();

ROLLBACK;
