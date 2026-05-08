//! Query surface for [`moniker`]: containment operators (`<@`, `@>`),
//! tree-position accessors (`parent_of`, `kind_of`, `path_of`), and
//! child composition (`compose_child`).

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

/// Cross-file linkage match. See SPEC.md § Operators and the rules
/// documented on `crate::core::moniker::MonikerView::bind_match`.
/// Exposed both as a function (for explicit calls) and as the `?=`
/// operator registered in the moniker GiST opclass at strategy 11.
/// Operates on borrowed views to avoid per-row clones at JOIN scale.
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

/// SPEC `parent || (kind, name)` exposed as a function — pgrx 0.18
/// has no row-composite RHS for `||`.
#[pg_extern(immutable, parallel_safe)]
fn compose_child(parent: moniker, kind: &str, name: &str) -> moniker {
	let mut b = MonikerBuilder::from_view(parent.view());
	b.segment(kind.as_bytes(), name.as_bytes());
	moniker::from_core(b.build())
}

/// Name of the first `external_pkg:<root>` segment, or NULL when the
/// moniker is project-local (no segment of kind `external_pkg`).
/// Joins consumer linkage tables onto extracted refs without scanning
/// the full segment list per row.
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
