//! Linkage census harness: loads a workspace like the acceptance tests do and
//! dumps the raw material needed to diagnose resolution gaps — every
//! unresolved reference with its extraction context, every resolved reference
//! whose extractor confidence is a name-based fallback, and global counters by
//! (status, confidence, kind). Output is JSONL on the path given as the second
//! argument, meant for offline classification.
//!
//! Usage: cargo run -p code-moniker-workspace --release --example linkage_census -- <workspace_root> <out.jsonl>

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceSnapshot};
use code_moniker_workspace::{LocalWorkspaceOptions, LocalWorkspaceRegistry};

fn main() {
	let mut args = std::env::args().skip(1);
	let root = PathBuf::from(
		args.next()
			.expect("usage: linkage_census <root> <out.jsonl>"),
	);
	let out_path = PathBuf::from(
		args.next()
			.expect("usage: linkage_census <root> <out.jsonl>"),
	);
	let snapshot = load(root);
	dump(&snapshot, &out_path);
}

fn load(root: PathBuf) -> WorkspaceSnapshot {
	let options = LocalWorkspaceOptions::new(vec![root], None);
	let mut workspace = LocalWorkspaceRegistry::local(options);
	let transition = workspace
		.commands()
		.refresh(WorkspaceRequest::new("linkage-census"));
	assert!(
		matches!(
			transition,
			code_moniker_workspace::snapshot::WorkspaceTransition::Ready { .. }
		),
		"workspace refresh failed: {transition:?}"
	);
	workspace
		.queries()
		.snapshot()
		.expect("ready workspace should expose a snapshot")
		.clone()
}

fn dump(snapshot: &WorkspaceSnapshot, out_path: &PathBuf) {
	let mut out = std::io::BufWriter::new(std::fs::File::create(out_path).expect("create output"));
	let symbols_by_id: HashMap<_, _> = snapshot
		.index
		.symbols
		.iter()
		.map(|symbol| (symbol.id, symbol))
		.collect();
	let unresolved_by_ref: HashMap<_, _> = snapshot
		.linkage
		.unresolved
		.iter()
		.map(|item| (item.reference, item))
		.collect();
	let blocked_by_ref: HashMap<_, _> = snapshot
		.linkage
		.manifest_blocked
		.iter()
		.map(|item| (item.reference, item))
		.collect();
	let external_by_ref: HashMap<_, _> = snapshot
		.linkage
		.external
		.iter()
		.map(|item| (item.reference, item))
		.collect();
	let resolved_by_ref: HashMap<_, _> = snapshot
		.linkage
		.resolved
		.iter()
		.map(|edge| (edge.reference, edge))
		.collect();

	let mut counters: HashMap<(String, String, String), usize> = HashMap::new();
	for reference in snapshot.index.references.iter() {
		let status = if resolved_by_ref.contains_key(&reference.id) {
			"resolved"
		} else if external_by_ref.contains_key(&reference.id) {
			"external"
		} else if blocked_by_ref.contains_key(&reference.id) {
			"blocked"
		} else if unresolved_by_ref.contains_key(&reference.id) {
			"unresolved"
		} else {
			"untracked"
		};
		let confidence = reference.confidence.clone().unwrap_or_default();
		*counters
			.entry((status.to_string(), confidence, reference.kind.clone()))
			.or_default() += 1;

		let dump_detail = status == "unresolved"
			|| (status == "resolved" && reference.confidence.as_deref() == Some("name_match"));
		if !dump_detail {
			continue;
		}
		let source_identity = symbols_by_id
			.get(&reference.source_symbol)
			.map(|symbol| symbol.identity.as_ref())
			.unwrap_or("");
		let resolved_target = resolved_by_ref
			.get(&reference.id)
			.and_then(|edge| symbols_by_id.get(&edge.target))
			.map(|symbol| symbol.identity.as_ref())
			.unwrap_or("");
		let record = serde_json::json!({
			"status": status,
			"kind": reference.kind,
			"call_name": reference.call_name,
			"call_arity": reference.call_arity,
			"confidence": reference.confidence,
			"receiver": reference.receiver,
			"target": reference.target_identity.as_ref(),
			"source_symbol": source_identity,
			"resolved_target": resolved_target,
		});
		writeln!(out, "{record}").expect("write record");
	}

	let mut counter_rows: Vec<_> = counters.into_iter().collect();
	counter_rows.sort_by_key(|row| std::cmp::Reverse(row.1));
	for ((status, confidence, kind), count) in counter_rows {
		let record = serde_json::json!({
			"status": "counter",
			"bucket_status": status,
			"confidence": confidence,
			"kind": kind,
			"count": count,
		});
		writeln!(out, "{record}").expect("write counter");
	}
}
