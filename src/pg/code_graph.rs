//! PostgreSQL type wrapping [`crate::core::code_graph::CodeGraph`].
//!
//! Constructors clone the graph and return a new value; the type is
//! immutable from SQL.

use pgrx::iter::TableIterator;
use pgrx::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::code_graph::CodeGraph as CoreGraph;
use crate::pg::moniker::moniker;

#[allow(non_camel_case_types)]
#[derive(PostgresType, Serialize, Deserialize, Clone, Debug)]
pub struct code_graph {
	inner: CoreGraph,
}

impl code_graph {
	pub(super) fn from_core(inner: CoreGraph) -> Self {
		Self { inner }
	}
}

#[pg_extern(immutable, parallel_safe)]
fn graph_create(root: moniker, kind: &str) -> code_graph {
	code_graph::from_core(CoreGraph::new(root.to_core(), kind.as_bytes()))
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_def(graph: code_graph, def: moniker, kind: &str, parent: moniker) -> code_graph {
	let mut next = graph.inner.clone();
	next.add_def(def.to_core(), kind.as_bytes(), &parent.to_core(), None)
		.unwrap_or_else(|e| error!("graph_add_def: {e}"));
	code_graph::from_core(next)
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_ref(
	graph: code_graph,
	source: moniker,
	target: moniker,
	kind: &str,
) -> code_graph {
	let mut next = graph.inner.clone();
	next.add_ref(&source.to_core(), target.to_core(), kind.as_bytes(), None)
		.unwrap_or_else(|e| error!("graph_add_ref: {e}"));
	code_graph::from_core(next)
}

#[pg_extern(immutable, parallel_safe)]
fn graph_root(graph: code_graph) -> moniker {
	moniker::from_core(graph.inner.root().clone())
}

#[pg_operator(immutable, parallel_safe)]
#[opname(@>)]
fn graph_contains(graph: code_graph, m: moniker) -> bool {
	graph.inner.contains(&m.to_core())
}

#[pg_extern(immutable, parallel_safe)]
fn graph_def_monikers(graph: code_graph) -> Vec<moniker> {
	graph
		.inner
		.def_monikers()
		.into_iter()
		.map(moniker::from_core)
		.collect()
}

#[pg_extern(immutable, parallel_safe)]
fn graph_ref_targets(graph: code_graph) -> Vec<moniker> {
	graph
		.inner
		.ref_targets()
		.into_iter()
		.map(moniker::from_core)
		.collect()
}

fn kind_text(bytes: &[u8]) -> String {
	String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| {
		error!("graph kind tag must be UTF-8");
	})
}

#[pg_extern(immutable, parallel_safe)]
fn graph_defs(
	graph: code_graph,
) -> TableIterator<'static, (name!(moniker, moniker), name!(kind, String))> {
	let rows: Vec<(moniker, String)> = graph
		.inner
		.defs()
		.map(|d| (moniker::from_core(d.moniker.clone()), kind_text(&d.kind)))
		.collect();
	TableIterator::new(rows.into_iter())
}

#[pg_extern(immutable, parallel_safe)]
fn graph_refs(
	graph: code_graph,
) -> TableIterator<
	'static,
	(
		name!(source, moniker),
		name!(target, moniker),
		name!(kind, String),
	),
> {
	let defs: Vec<_> = graph.inner.defs().collect();
	let rows: Vec<(moniker, moniker, String)> = graph
		.inner
		.refs()
		.map(|r| {
			let source_def = defs
				.get(r.source)
				.unwrap_or_else(|| error!("ref source index {} out of bounds", r.source));
			(
				moniker::from_core(source_def.moniker.clone()),
				moniker::from_core(r.target.clone()),
				kind_text(&r.kind),
			)
		})
		.collect();
	TableIterator::new(rows.into_iter())
}
