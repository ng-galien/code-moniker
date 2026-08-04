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
	let sources_by_id: HashMap<_, _> = snapshot
		.index
		.sources
		.iter()
		.map(|source| (source.id, source))
		.collect();
	let symbols_by_id: HashMap<_, _> = snapshot
		.index
		.symbols
		.iter()
		.map(|symbol| (symbol.id, symbol))
		.collect();
	let mut methods_by_call = HashMap::<(String, Option<usize>), Vec<&str>>::new();
	for symbol in snapshot.index.symbols.iter() {
		let Some(call_name) = &symbol.call_name else {
			continue;
		};
		methods_by_call
			.entry((call_name.clone(), symbol.call_arity))
			.or_default()
			.push(symbol.identity.as_ref());
	}
	let unresolved_by_ref: HashMap<_, _> = snapshot
		.linkage
		.unresolved
		.iter()
		.map(|item| (item.reference, item))
		.collect();
	let blocked_by_ref: HashMap<_, _> = snapshot
		.linkage
		.blocked
		.iter()
		.chain(snapshot.linkage.manifest_blocked.iter())
		.map(|item| (item.reference, item))
		.collect();
	let candidate_by_ref: HashMap<_, _> = snapshot
		.linkage
		.candidates
		.iter()
		.map(|item| (item.reference, item))
		.collect();
	let dynamic_by_ref: HashMap<_, _> = snapshot
		.linkage
		.dynamic
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

	let mut counters: HashMap<(String, String, String, String, String, String), usize> =
		HashMap::new();
	for reference in snapshot.index.references.iter() {
		let (status, reason, target_count) = classification(
			reference.id,
			&resolved_by_ref,
			&candidate_by_ref,
			&external_by_ref,
			&dynamic_by_ref,
			&blocked_by_ref,
			&unresolved_by_ref,
		);
		let language = sources_by_id
			.get(&reference.source)
			.map(|source| source.language.as_str())
			.unwrap_or("unknown");
		let confidence = reference.confidence.clone().unwrap_or_default();
		let receiver_shape = receiver_shape(reference.receiver.as_deref());
		*counters
			.entry((
				status.to_string(),
				reason.to_string(),
				language.to_string(),
				reference.kind.clone(),
				confidence.clone(),
				receiver_shape.to_string(),
			))
			.or_default() += 1;

		let dump_detail = matches!(
			status,
			"candidate" | "dynamic" | "blocked" | "unresolved" | "unclassified"
		) || (status == "resolved"
			&& reference.confidence.as_deref() == Some("name_match"));
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
		let candidates = candidate_by_ref
			.get(&reference.id)
			.map(|item| {
				item.targets
					.iter()
					.filter_map(|target| symbols_by_id.get(target))
					.map(|symbol| symbol.identity.as_ref())
					.collect::<Vec<_>>()
			})
			.or_else(|| {
				dynamic_by_ref.get(&reference.id).map(|item| {
					item.candidates
						.iter()
						.filter_map(|target| symbols_by_id.get(target))
						.map(|symbol| symbol.identity.as_ref())
						.collect::<Vec<_>>()
				})
			})
			.unwrap_or_default();
		let structural_method_candidates = reference
			.call_name
			.as_ref()
			.and_then(|name| methods_by_call.get(&(name.clone(), reference.call_arity)))
			.cloned()
			.unwrap_or_default();
		let record = serde_json::json!({
			"status": status,
			"reason": reason,
			"language": language,
			"kind": reference.kind,
			"call_name": reference.call_name,
			"call_arity": reference.call_arity,
			"confidence": reference.confidence,
			"receiver": reference.receiver,
			"receiver_shape": receiver_shape,
			"target": reference.target_identity.as_ref(),
			"target_count": target_count,
			"candidates": candidates,
			"structural_method_candidates": structural_method_candidates,
			"source_symbol": source_identity,
			"resolved_target": resolved_target,
		});
		writeln!(out, "{record}").expect("write record");
	}

	let mut counter_rows: Vec<_> = counters.into_iter().collect();
	counter_rows.sort_by_key(|row| std::cmp::Reverse(row.1));
	for ((status, reason, language, kind, confidence, receiver_shape), count) in counter_rows {
		let record = serde_json::json!({
			"status": "counter",
			"bucket_status": status,
			"reason": reason,
			"language": language,
			"confidence": confidence,
			"kind": kind,
			"receiver_shape": receiver_shape,
			"count": count,
		});
		writeln!(out, "{record}").expect("write counter");
	}
}

#[allow(clippy::too_many_arguments)]
fn classification<'a>(
	reference: code_moniker_workspace::snapshot::ReferenceId,
	resolved: &HashMap<
		code_moniker_workspace::snapshot::ReferenceId,
		&'a code_moniker_workspace::snapshot::LinkageEdge,
	>,
	candidates: &HashMap<
		code_moniker_workspace::snapshot::ReferenceId,
		&'a code_moniker_workspace::snapshot::CandidateReference,
	>,
	external: &HashMap<
		code_moniker_workspace::snapshot::ReferenceId,
		&'a code_moniker_workspace::snapshot::ExternalReference,
	>,
	dynamic: &HashMap<
		code_moniker_workspace::snapshot::ReferenceId,
		&'a code_moniker_workspace::snapshot::DynamicReference,
	>,
	blocked: &HashMap<
		code_moniker_workspace::snapshot::ReferenceId,
		&'a code_moniker_workspace::snapshot::UnresolvedReference,
	>,
	unresolved: &HashMap<
		code_moniker_workspace::snapshot::ReferenceId,
		&'a code_moniker_workspace::snapshot::UnresolvedReference,
	>,
) -> (&'static str, &'static str, usize) {
	if let Some(item) = resolved.get(&reference) {
		("resolved", item.evidence.as_str(), 1)
	} else if let Some(item) = candidates.get(&reference) {
		("candidate", item.reason.as_str(), item.targets.len())
	} else if let Some(item) = external.get(&reference) {
		("external", item.origin.label(), 1)
	} else if let Some(item) = dynamic.get(&reference) {
		("dynamic", item.reason.as_str(), item.candidates.len())
	} else if let Some(item) = blocked.get(&reference) {
		("blocked", item.reason.as_str(), 0)
	} else if let Some(item) = unresolved.get(&reference) {
		("unresolved", item.reason.as_str(), 0)
	} else {
		("unclassified", "missing_final_decision", 0)
	}
}

fn receiver_shape(receiver: Option<&str>) -> &'static str {
	match receiver {
		None | Some("") => "none",
		Some("self" | "cls") => "self",
		Some("member" | "subscript") => "member",
		Some("call") => "call",
		Some(value)
			if value
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') =>
		{
			"identifier"
		}
		Some(_) => "compound",
	}
}
