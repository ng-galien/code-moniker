use pgrx::prelude::*;

use super::moniker;
use crate::core::moniker::MonikerBuilder;

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

#[pg_operator(immutable, parallel_safe)]
#[opname(?=)]
#[restrict(eqsel)]
#[join(eqjoinsel)]
fn bind_match(a: moniker, b: moniker) -> bool {
	a.view().bind_match(&b.view())
}

#[pg_extern(immutable, parallel_safe)]
fn parent_of(m: moniker) -> Option<moniker> {
	m.to_core().parent().map(moniker::from_core)
}

#[pg_extern(immutable, parallel_safe)]
fn kind_of(m: moniker) -> Option<String> {
	m.view().segments().last().map(|s| {
		std::str::from_utf8(s.kind)
			.unwrap_or_else(|_| error!("moniker kind must be UTF-8"))
			.to_string()
	})
}

#[pg_extern(immutable, parallel_safe)]
fn path_of(m: moniker) -> Vec<String> {
	m.view()
		.segments()
		.map(|s| {
			std::str::from_utf8(s.name)
				.unwrap_or_else(|_| error!("moniker segment name must be UTF-8"))
				.to_string()
		})
		.collect()
}

#[pg_extern(immutable, parallel_safe)]
fn compose_child(parent: moniker, kind: &str, name: &str) -> moniker {
	let mut b = MonikerBuilder::from_view(parent.view());
	b.segment(kind.as_bytes(), name.as_bytes());
	moniker::from_core(b.build())
}

#[pg_operator(immutable, parallel_safe)]
#[opname(||)]
fn compose_child_typed(parent: moniker, segment: &str) -> moniker {
	let (kind, name) = match segment.split_once(':') {
		Some((k, n)) if !k.is_empty() && !n.is_empty() => (k, n),
		_ => error!("|| expects RHS in 'kind:name' form, got {segment:?}"),
	};
	compose_child(parent, kind, name)
}

#[pg_extern(immutable, parallel_safe)]
fn bare_callable_name(m: moniker) -> moniker {
	moniker::from_core(m.to_core().with_bare_last_segment())
}

#[pg_extern(immutable, parallel_safe)]
fn external_pkg_root(m: moniker) -> Option<String> {
	m.view()
		.segments()
		.find(|s| s.kind == b"external_pkg")
		.map(|s| {
			std::str::from_utf8(s.name)
				.unwrap_or_else(|_| error!("external_pkg root must be UTF-8"))
				.to_string()
		})
}
