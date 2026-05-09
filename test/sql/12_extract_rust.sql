
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(11);

SELECT has_function('extract_rust'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_rust(text, text, moniker, boolean) is exposed');

WITH g AS (
	SELECT extract_rust(
		'src/core/moniker/view.rs',
		'',
		'pcm+moniker://pg_code_moniker'::moniker
	) AS g
)
SELECT is(
	graph_root(g)::text,
	'pcm+moniker://pg_code_moniker/lang:rs/dir:src/dir:core/dir:moniker/module:view',
	'module moniker = anchor + path:dir + module:basename')
FROM g;

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
		'pcm+moniker://pkg'::moniker
	) AS g
)
SELECT
	ok(g @> 'pcm+moniker://pkg/lang:rs/module:util/struct:Foo'::moniker,
		'struct emits struct def') AS r1,
	ok(g @> 'pcm+moniker://pkg/lang:rs/module:util/struct:Foo/method:bar()'::moniker,
		'impl method re-parented onto struct; &self is implicit so arity 0 with empty parens') AS r2,
	ok(g @> 'pcm+moniker://pkg/lang:rs/module:util/struct:Foo/method:baz(u32)'::moniker,
		'second impl method with one value parameter (self excluded)') AS r3
FROM g;

WITH g AS (
	SELECT extract_rust(
		'util.rs',
		$rs$
pub trait Greet { fn hi(&self); }
pub struct Foo;
impl Greet for Foo { fn hi(&self) {} }
$rs$,
		'pcm+moniker://pkg'::moniker
	) AS g
)
SELECT
	ok(EXISTS (
		SELECT 1 FROM graph_refs(g) r
		 WHERE r.kind = 'implements'
		   AND r.source = 'pcm+moniker://pkg/lang:rs/module:util/struct:Foo'::moniker
		   AND r.target = 'pcm+moniker://pkg/lang:rs/module:util/trait:Greet'::moniker),
		'impl Trait for Type emits implements ref Type → Trait') AS r4
FROM g;

WITH g AS (
	SELECT extract_rust(
		'util.rs',
		'use std::collections::{HashMap, HashSet};',
		'pcm+moniker://pkg'::moniker
	) AS g
)
SELECT
	is(array_length(graph_ref_targets(g), 1), 2,
		'group import emits one ref per leaf') AS r5,
	ok('pcm+moniker://pkg/external_pkg:std/path:collections/path:HashMap'::moniker
	     = ANY(graph_ref_targets(g)),
		'first leaf reaches HashMap under external_pkg:std') AS r6,
	ok('pcm+moniker://pkg/external_pkg:std/path:collections/path:HashSet'::moniker
	     = ANY(graph_ref_targets(g)),
		'second leaf reaches HashSet under external_pkg:std') AS r7
FROM g;

WITH g AS (
	SELECT extract_rust(
		'util.rs',
		$rs$
use crate::core::moniker::Moniker;
use pgrx::prelude::*;
$rs$,
		'pcm+moniker://pkg'::moniker
	) AS g
)
SELECT
	ok('pcm+moniker://pkg/lang:rs/dir:core/module:moniker/path:Moniker'::moniker
	     = ANY(graph_ref_targets(g)),
		'crate:: prefix resolves under the project anchor (no external_pkg)') AS r8,
	ok('pcm+moniker://pkg/external_pkg:pgrx/path:prelude'::moniker
	     = ANY(graph_ref_targets(g)),
		'bare external crate root marked with external_pkg:') AS r9
FROM g;

SELECT * FROM finish();

ROLLBACK;
