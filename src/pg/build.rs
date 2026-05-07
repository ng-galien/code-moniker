//! SQL primitives for build-system manifest extraction. Consumers
//! ingest these rows into their own linkage tables; the extension
//! itself stays stateless.

use pgrx::iter::TableIterator;
use pgrx::prelude::*;

use crate::lang::rs::build as cargo;

#[pg_extern(immutable, parallel_safe)]
fn extract_cargo(
	content: &str,
) -> TableIterator<
	'static,
	(
		name!(name, String),
		name!(version, Option<String>),
		name!(dep_kind, String),
		name!(import_root, String),
	),
> {
	let deps = cargo::parse(content).unwrap_or_else(|e| error!("{e}"));
	let rows = deps
		.into_iter()
		.map(|d| (d.name, d.version, d.dep_kind, d.import_root))
		.collect::<Vec<_>>();
	TableIterator::new(rows.into_iter())
}
