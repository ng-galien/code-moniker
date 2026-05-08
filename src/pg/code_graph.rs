use pgrx::iter::TableIterator;
use pgrx::prelude::*;
use pgrx::{default, name};
use serde::{Deserialize, Serialize};

use crate::core::code_graph::{CodeGraph as CoreGraph, Position};
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
	code_graph::from_core(CoreGraph::new(root.into_core(), kind.as_bytes()))
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_def(
	graph: code_graph,
	def: moniker,
	kind: &str,
	parent: moniker,
	start_byte: default!(Option<i32>, "NULL"),
	end_byte: default!(Option<i32>, "NULL"),
) -> code_graph {
	let mut next = graph.inner.clone();
	next.add_def(def.into_core(), kind.as_bytes(), &parent.to_core(), pos_from_args(start_byte, end_byte))
		.unwrap_or_else(|e| error!("graph_add_def: {e}"));
	code_graph::from_core(next)
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_ref(
	graph: code_graph,
	source: moniker,
	target: moniker,
	kind: &str,
	start_byte: default!(Option<i32>, "NULL"),
	end_byte: default!(Option<i32>, "NULL"),
) -> code_graph {
	let mut next = graph.inner.clone();
	next.add_ref(&source.to_core(), target.into_core(), kind.as_bytes(), pos_from_args(start_byte, end_byte))
		.unwrap_or_else(|e| error!("graph_add_ref: {e}"));
	code_graph::from_core(next)
}

fn pos_from_args(start: Option<i32>, end: Option<i32>) -> Option<Position> {
	match (start, end) {
		(Some(s), Some(e)) if s >= 0 && e >= 0 => Some((s as u32, e as u32)),
		_ => None,
	}
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_defs(
	graph: code_graph,
	defs: Vec<moniker>,
	kinds: Vec<String>,
	parents: Vec<moniker>,
) -> code_graph {
	if defs.len() != kinds.len() || defs.len() != parents.len() {
		error!("graph_add_defs: arrays must have the same length");
	}
	let mut next = graph.inner.clone();
	for ((d, k), p) in defs.into_iter().zip(kinds.into_iter()).zip(parents.into_iter()) {
		next.add_def(d.into_core(), k.as_bytes(), &p.to_core(), None)
			.unwrap_or_else(|e| error!("graph_add_defs: {e}"));
	}
	code_graph::from_core(next)
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_refs(
	graph: code_graph,
	sources: Vec<moniker>,
	targets: Vec<moniker>,
	kinds: Vec<String>,
) -> code_graph {
	if sources.len() != targets.len() || sources.len() != kinds.len() {
		error!("graph_add_refs: arrays must have the same length");
	}
	let mut next = graph.inner.clone();
	for ((s, t), k) in sources.into_iter().zip(targets.into_iter()).zip(kinds.into_iter()) {
		next.add_ref(&s.to_core(), t.into_core(), k.as_bytes(), None)
			.unwrap_or_else(|e| error!("graph_add_refs: {e}"));
	}
	code_graph::from_core(next)
}

#[pg_extern(immutable, parallel_safe)]
fn graph_locate(
	graph: code_graph,
	m: moniker,
) -> TableIterator<'static, (name!(start_byte, Option<i32>), name!(end_byte, Option<i32>))> {
	let row = graph.inner.locate(&m.to_core()).map(|p| {
		let (s, e) = position_to_i32(Some(p));
		(s, e)
	});
	TableIterator::new(row.into_iter())
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
		.iter()
		.map(|m| moniker::from_core(m.clone()))
		.collect()
}

#[pg_extern(immutable, parallel_safe)]
fn graph_ref_targets(graph: code_graph) -> Vec<moniker> {
	graph
		.inner
		.ref_targets()
		.iter()
		.map(|m| moniker::from_core(m.clone()))
		.collect()
}

#[pg_extern(immutable, parallel_safe)]
fn graph_export_monikers(graph: code_graph) -> Vec<moniker> {
	use crate::core::kinds::{BIND_EXPORT, BIND_INJECT};
	let mut core: Vec<crate::core::moniker::Moniker> = graph
		.inner
		.defs()
		.filter(|d| d.binding == BIND_EXPORT || d.binding == BIND_INJECT)
		.map(|d| d.moniker.clone())
		.collect();
	core.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
	core.into_iter().map(moniker::from_core).collect()
}

#[pg_extern(immutable, parallel_safe)]
fn graph_import_targets(graph: code_graph) -> Vec<moniker> {
	use crate::core::kinds::{BIND_IMPORT, BIND_INJECT};
	let mut core: Vec<crate::core::moniker::Moniker> = graph
		.inner
		.refs()
		.filter(|r| r.binding == BIND_IMPORT || r.binding == BIND_INJECT)
		.map(|r| r.target.clone())
		.collect();
	core.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
	core.into_iter().map(moniker::from_core).collect()
}

fn kind_text(bytes: &[u8]) -> String {
	String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| {
		error!("graph kind tag must be UTF-8");
	})
}

#[pg_extern(immutable, parallel_safe)]
fn graph_defs(
	graph: code_graph,
) -> TableIterator<
	'static,
	(
		name!(moniker, moniker),
		name!(kind, String),
		name!(visibility, Option<String>),
		name!(signature, Option<String>),
		name!(binding, Option<String>),
		name!(start_byte, Option<i32>),
		name!(end_byte, Option<i32>),
	),
> {
	let rows: Vec<(
		moniker,
		String,
		Option<String>,
		Option<String>,
		Option<String>,
		Option<i32>,
		Option<i32>,
	)> = graph
		.inner
		.defs()
		.map(|d| {
			let (start, end) = position_to_i32(d.position);
			(
				moniker::from_core(d.moniker.clone()),
				kind_text(&d.kind),
				bytes_to_opt_string(&d.visibility),
				bytes_to_opt_string(&d.signature),
				bytes_to_opt_string(&d.binding),
				start,
				end,
			)
		})
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
		name!(receiver_hint, Option<String>),
		name!(alias, Option<String>),
		name!(confidence, Option<String>),
		name!(binding, Option<String>),
		name!(start_byte, Option<i32>),
		name!(end_byte, Option<i32>),
	),
> {
	let defs: Vec<_> = graph.inner.defs().collect();
	let rows: Vec<(
		moniker,
		moniker,
		String,
		Option<String>,
		Option<String>,
		Option<String>,
		Option<String>,
		Option<i32>,
		Option<i32>,
	)> = graph
		.inner
		.refs()
		.map(|r| {
			let source_def = defs
				.get(r.source)
				.unwrap_or_else(|| error!("ref source index {} out of bounds", r.source));
			let (start, end) = position_to_i32(r.position);
			(
				moniker::from_core(source_def.moniker.clone()),
				moniker::from_core(r.target.clone()),
				kind_text(&r.kind),
				bytes_to_opt_string(&r.receiver_hint),
				bytes_to_opt_string(&r.alias),
				bytes_to_opt_string(&r.confidence),
				bytes_to_opt_string(&r.binding),
				start,
				end,
			)
		})
		.collect();
	TableIterator::new(rows.into_iter())
}

fn bytes_to_opt_string(b: &[u8]) -> Option<String> {
	(!b.is_empty()).then(|| kind_text(b))
}

fn position_to_i32(p: Option<Position>) -> (Option<i32>, Option<i32>) {
	let clamp = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
	match p {
		None => (None, None),
		Some((s, e)) => (Some(clamp(s)), Some(clamp(e))),
	}
}
