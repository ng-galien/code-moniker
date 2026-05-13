use pgrx::prelude::*;

use crate::code_graph::code_graph;
use crate::moniker::moniker;

#[pg_extern(immutable, parallel_safe)]
fn extract_typescript(
	uri: &str,
	source: &str,
	anchor: moniker,
	deep: pgrx::default!(bool, "false"),
	di_register_callees: pgrx::default!(Vec<String>, "ARRAY[]::text[]"),
) -> code_graph {
	let core_anchor = anchor.to_core();
	let presets = code_moniker_core::lang::ts::Presets {
		di_register_callees,
	};
	let inner = code_moniker_core::lang::ts::extract(uri, source, &core_anchor, deep, &presets);
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
	let presets = code_moniker_core::lang::rs::Presets::default();
	let inner = code_moniker_core::lang::rs::extract(uri, source, &core_anchor, deep, &presets);
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
	let presets = code_moniker_core::lang::java::Presets::default();
	let inner = code_moniker_core::lang::java::extract(uri, source, &core_anchor, deep, &presets);
	code_graph::from_core(inner)
}

#[pg_extern]
fn extract_plpgsql(
	uri: &str,
	source: &str,
	anchor: moniker,
	deep: pgrx::default!(bool, "false"),
) -> code_graph {
	let core_anchor = anchor.to_core();
	let presets = code_moniker_core::lang::sql::Presets::default();
	let inner = code_moniker_core::lang::sql::extract(uri, source, &core_anchor, deep, &presets);
	code_graph::from_core(inner)
}

#[pg_extern(immutable, parallel_safe)]
fn extract_python(
	uri: &str,
	source: &str,
	anchor: moniker,
	deep: pgrx::default!(bool, "false"),
) -> code_graph {
	let core_anchor = anchor.to_core();
	let presets = code_moniker_core::lang::python::Presets::default();
	let inner = code_moniker_core::lang::python::extract(uri, source, &core_anchor, deep, &presets);
	code_graph::from_core(inner)
}

#[pg_extern(immutable, parallel_safe)]
fn extract_go(
	uri: &str,
	source: &str,
	anchor: moniker,
	deep: pgrx::default!(bool, "false"),
) -> code_graph {
	let core_anchor = anchor.to_core();
	let presets = code_moniker_core::lang::go::Presets::default();
	let inner = code_moniker_core::lang::go::extract(uri, source, &core_anchor, deep, &presets);
	code_graph::from_core(inner)
}

#[pg_extern(immutable, parallel_safe)]
fn extract_csharp(
	uri: &str,
	source: &str,
	anchor: moniker,
	deep: pgrx::default!(bool, "false"),
) -> code_graph {
	let core_anchor = anchor.to_core();
	let presets = code_moniker_core::lang::cs::Presets::default();
	let inner = code_moniker_core::lang::cs::extract(uri, source, &core_anchor, deep, &presets);
	code_graph::from_core(inner)
}
