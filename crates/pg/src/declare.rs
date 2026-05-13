use std::error::Error;

use pgrx::prelude::*;

use crate::code_graph::code_graph;
use code_moniker_core::declare::{declare_from_json_value, graph_to_spec};

type PgError = Box<dyn Error + Send + Sync + 'static>;

#[pg_extern(immutable, parallel_safe)]
fn code_graph_declare(spec: pgrx::JsonB) -> Result<code_graph, PgError> {
	Ok(code_graph::from_core(declare_from_json_value(&spec.0)?))
}

#[pg_extern(immutable, parallel_safe)]
fn code_graph_to_spec(graph: code_graph) -> Result<pgrx::JsonB, PgError> {
	Ok(pgrx::JsonB(graph_to_spec(&graph.to_core())?))
}
