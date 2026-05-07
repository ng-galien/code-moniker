//! Phase 4 query surface for [`moniker`]: containment operators
//! (`<@`, `@>`), tree-position accessors (`parent_of`, `kind_of`,
//! `path_of`), and child composition (`compose_child`).

use pgrx::prelude::*;

use super::moniker;
use crate::core::moniker::MonikerBuilder;
use crate::pg::registry::{intern_kind, kind_name, punct_for_kind};

#[pg_operator(immutable, parallel_safe)]
#[opname(<@)]
fn moniker_descendant_of(a: moniker, b: moniker) -> bool {
	b.view().is_ancestor_of(&a.view())
}

#[pg_operator(immutable, parallel_safe)]
#[opname(@>)]
fn moniker_ancestor_of(a: moniker, b: moniker) -> bool {
	a.view().is_ancestor_of(&b.view())
}

#[pg_extern(immutable, parallel_safe)]
fn parent_of(m: moniker) -> Option<moniker> {
	m.to_core().parent().map(moniker::from_core)
}

#[pg_extern(immutable, parallel_safe)]
fn kind_of(m: moniker) -> Option<String> {
	m.view().segments().last().map(|s| kind_name(s.kind))
}

#[pg_extern(immutable, parallel_safe)]
fn path_of(m: moniker) -> Vec<String> {
	m.view()
		.segments()
		.map(|s| std::str::from_utf8(s.bytes).expect("segment must be UTF-8").to_string())
		.collect()
}

/// SPEC `parent || (segment, kind)` exposed as a function — pgrx 0.18
/// has no row-composite RHS for `||`.
#[pg_extern(immutable, parallel_safe)]
fn compose_child(parent: moniker, segment: &str, kind: &str) -> moniker {
	let kind_id = intern_kind(kind, punct_for_kind(kind));
	let mut b = MonikerBuilder::from_view(parent.view());
	b.segment(kind_id, segment.as_bytes());
	moniker::from_core(b.build())
}
