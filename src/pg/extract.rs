//! Per-language extraction entry points exposed to SQL.

use pgrx::prelude::*;

use crate::pg::code_graph::code_graph;
use crate::pg::moniker::moniker;

#[pg_extern(immutable, parallel_safe)]
fn extract_typescript(
	uri: &str,
	source: &str,
	anchor: moniker,
	deep: pgrx::default!(bool, "false"),
	di_register_callees: pgrx::default!(Vec<String>, "ARRAY[]::text[]"),
) -> code_graph {
	let core_anchor = anchor.to_core();
	let presets = crate::lang::ts::Presets {
		di_register_callees,
	};
	let inner = crate::lang::ts::extract(uri, source, &core_anchor, deep, &presets);
	code_graph::from_core(inner)
}

#[pg_extern(immutable, parallel_safe)]
fn extract_rust(
	uri: &str,
	source: &str,
	anchor: moniker,
	deep: pgrx::default!(bool, "false"),
) -> code_graph {
	let core_anchor = anchor.to_core();
	let inner = crate::lang::rs::extract(uri, source, &core_anchor, deep);
	code_graph::from_core(inner)
}

#[pg_extern(immutable, parallel_safe)]
fn extract_java(
	uri: &str,
	source: &str,
	anchor: moniker,
	deep: pgrx::default!(bool, "false"),
) -> code_graph {
	let core_anchor = anchor.to_core();
	let presets = crate::lang::java::Presets::default();
	let inner = crate::lang::java::extract(uri, source, &core_anchor, deep, &presets);
	code_graph::from_core(inner)
}
