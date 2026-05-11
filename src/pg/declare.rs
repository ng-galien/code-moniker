use pgrx::prelude::*;

use crate::declare::{declare_from_json_value, graph_to_spec};
use crate::pg::code_graph::code_graph;

#[pg_extern(immutable, parallel_safe)]
fn code_graph_declare(spec: pgrx::JsonB) -> code_graph {
	match declare_from_json_value(&spec.0) {
		Ok(g) => code_graph::from_core(g),
		Err(e) => error!("code_graph_declare: {e}"),
	}
}

#[pg_extern(immutable, parallel_safe)]
fn code_graph_to_spec(graph: code_graph) -> pgrx::JsonB {
	let core = graph.to_core();
	match graph_to_spec(&core) {
		Ok(v) => pgrx::JsonB(v),
		Err(e) => error!("code_graph_to_spec: {e}"),
	}
}
