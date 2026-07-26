use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::snapshot::{ReferenceRecord, WorkspaceSnapshot};

// Embedded resolution audit: every reference is partitioned by decision class;
// candidate, dynamic, and unresolved decisions are classified under mechanical
// pattern keys. Labels are facts about the reference, never guesses at a cause.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolutionAudit {
	pub totals: AuditTotals,
	pub clusters: Vec<AuditCluster>,
	pub zones: Vec<AuditZone>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditTotals {
	pub references: usize,
	/// Compatibility alias for `unique`.
	pub resolved: usize,
	pub unique: usize,
	pub candidate: usize,
	pub external: usize,
	pub sdk: usize,
	pub dependency: usize,
	pub injected_external: usize,
	pub unknown_external: usize,
	pub dynamic: usize,
	pub blocked: usize,
	pub unresolved: usize,
	pub explained: usize,
	pub weak_or_unexplained: usize,
	pub name_match_resolved: usize,
	pub name_match_candidate: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditCluster {
	pub id: String,
	pub pattern: AuditPattern,
	pub count: usize,
	pub samples: Vec<AuditSample>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct AuditPattern {
	pub status: String,
	pub reason: String,
	pub evidence: String,
	pub confidence: String,
	pub kind: String,
	pub receiver: String,
	pub target_shape: String,
	pub target_head: String,
	pub srcset: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditSample {
	pub file: String,
	pub line_range: Option<(u32, u32)>,
	pub snippet: String,
	pub source: String,
	pub call_name: String,
	pub receiver: String,
	pub target: String,
	pub evidence: String,
	pub constraints: Vec<String>,
	pub candidates: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditZone {
	pub zone: String,
	pub unresolved: usize,
	pub dominant_pattern: String,
}

#[derive(Clone, Debug)]
pub struct AuditOptions {
	pub cluster_limit: usize,
	pub sample_limit: usize,
	pub sample_offset: usize,
	pub zone_limit: usize,
	pub cluster: Option<String>,
}

impl Default for AuditOptions {
	fn default() -> Self {
		Self {
			cluster_limit: 20,
			sample_limit: 3,
			sample_offset: 0,
			zone_limit: 10,
			cluster: None,
		}
	}
}

struct AuditLookups<'a> {
	symbol_identities: HashMap<crate::snapshot::SymbolId, &'a str>,
	source_paths: HashMap<crate::snapshot::SourceId, &'a str>,
	source_texts: HashMap<crate::snapshot::SourceId, &'a str>,
	unresolved: HashMap<crate::snapshot::ReferenceId, &'static str>,
	blocked: HashMap<crate::snapshot::ReferenceId, &'static str>,
	external: HashMap<crate::snapshot::ReferenceId, crate::snapshot::ExternalReferenceOrigin>,
	resolved: HashMap<
		crate::snapshot::ReferenceId,
		(
			crate::snapshot::ResolutionEvidence,
			crate::snapshot::SymbolId,
		),
	>,
	candidates: HashMap<crate::snapshot::ReferenceId, &'a crate::snapshot::CandidateReference>,
	dynamic: HashMap<crate::snapshot::ReferenceId, &'a crate::snapshot::DynamicReference>,
}

impl<'a> AuditLookups<'a> {
	fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self {
			symbol_identities: symbol_identities(&snapshot.index.symbols),
			source_paths: source_paths(&snapshot.index.sources),
			source_texts: source_texts(&snapshot.index.sources),
			unresolved: unresolved_reasons(&snapshot.linkage.unresolved),
			blocked: blocked_reasons(snapshot),
			external: external_references(&snapshot.linkage.external),
			resolved: resolved_evidence(&snapshot.linkage.resolved),
			candidates: candidate_references(&snapshot.linkage.candidates),
			dynamic: dynamic_references(&snapshot.linkage.dynamic),
		}
	}
}

fn symbol_identities(
	symbols: &crate::snapshot::RecordTable<crate::snapshot::SymbolRecord>,
) -> HashMap<crate::snapshot::SymbolId, &str> {
	symbols
		.iter()
		.map(|symbol| (symbol.id, symbol.identity.as_ref()))
		.collect()
}

fn source_paths(
	sources: &[crate::snapshot::SourceFileRecord],
) -> HashMap<crate::snapshot::SourceId, &str> {
	sources
		.iter()
		.map(|source| (source.id, source.rel_path.as_str()))
		.collect()
}

fn source_texts(
	sources: &[crate::snapshot::SourceFileRecord],
) -> HashMap<crate::snapshot::SourceId, &str> {
	sources
		.iter()
		.map(|source| (source.id, source.text.as_str()))
		.collect()
}

fn unresolved_reasons(
	references: &[crate::snapshot::UnresolvedReference],
) -> HashMap<crate::snapshot::ReferenceId, &'static str> {
	references
		.iter()
		.map(|item| (item.reference, item.reason.as_str()))
		.collect()
}

fn blocked_reasons(
	snapshot: &WorkspaceSnapshot,
) -> HashMap<crate::snapshot::ReferenceId, &'static str> {
	snapshot
		.linkage
		.blocked
		.iter()
		.chain(snapshot.linkage.manifest_blocked.iter())
		.map(|item| (item.reference, item.reason.as_str()))
		.collect()
}

fn external_references(
	references: &[crate::snapshot::ExternalReference],
) -> HashMap<crate::snapshot::ReferenceId, crate::snapshot::ExternalReferenceOrigin> {
	references
		.iter()
		.map(|item| (item.reference, item.origin))
		.collect()
}

fn resolved_evidence(
	edges: &[crate::snapshot::LinkageEdge],
) -> HashMap<
	crate::snapshot::ReferenceId,
	(
		crate::snapshot::ResolutionEvidence,
		crate::snapshot::SymbolId,
	),
> {
	edges
		.iter()
		.map(|edge| (edge.reference, (edge.evidence, edge.target)))
		.collect()
}

fn candidate_references(
	references: &[crate::snapshot::CandidateReference],
) -> HashMap<crate::snapshot::ReferenceId, &crate::snapshot::CandidateReference> {
	references
		.iter()
		.map(|candidate| (candidate.reference, candidate))
		.collect()
}

fn dynamic_references(
	references: &[crate::snapshot::DynamicReference],
) -> HashMap<crate::snapshot::ReferenceId, &crate::snapshot::DynamicReference> {
	references
		.iter()
		.map(|dynamic| (dynamic.reference, dynamic))
		.collect()
}

struct AuditClassification {
	status: &'static str,
	reason: &'static str,
	evidence: &'static str,
	scope: &'static str,
	candidate_targets: Vec<String>,
}

pub fn resolution_audit(
	snapshot: &WorkspaceSnapshot,
	prefix: &str,
	options: AuditOptions,
) -> ResolutionAudit {
	let lookups = AuditLookups::new(snapshot);
	let mut totals = AuditTotals::default();
	let mut clusters: HashMap<AuditPattern, (usize, Vec<AuditSample>)> = HashMap::new();
	let mut zones: HashMap<String, (usize, HashMap<String, usize>)> = HashMap::new();

	for reference in snapshot.index.references.iter() {
		let source = lookups
			.symbol_identities
			.get(&reference.source_symbol)
			.copied()
			.unwrap_or_default();
		if !prefix.is_empty() && !source.contains(prefix) {
			continue;
		}
		totals.references += 1;
		let Some(classification) = classify_reference(&lookups, reference, &mut totals) else {
			continue;
		};

		let pattern = pattern_for(
			classification.status,
			classification.reason,
			classification.evidence,
			reference,
			source,
		);
		let unresolved_cluster = classification.status == "unresolved";
		let cluster_id = pattern_id(&pattern);
		if options
			.cluster
			.as_deref()
			.is_some_and(|expected| expected != cluster_id.as_str())
		{
			continue;
		}
		let entry = clusters.entry(pattern.clone()).or_default();
		let sample_index = entry.0;
		entry.0 += 1;
		if sample_index >= options.sample_offset && entry.1.len() < options.sample_limit {
			entry.1.push(sample_for(
				reference,
				source,
				lookups
					.source_paths
					.get(&reference.source)
					.copied()
					.unwrap_or_default(),
				lookups
					.source_texts
					.get(&reference.source)
					.copied()
					.unwrap_or_default(),
				classification,
			));
		}
		if unresolved_cluster {
			let zone = zone_of(source);
			let slot = zones.entry(zone).or_default();
			slot.0 += 1;
			*slot.1.entry(pattern_label(&pattern)).or_default() += 1;
		}
	}
	totals.resolved = totals.unique;
	debug_assert_eq!(
		totals.external,
		totals.sdk + totals.dependency + totals.injected_external + totals.unknown_external,
		"external compatibility total must equal its provenance buckets"
	);
	totals.explained = totals.unique
		+ totals.candidate
		+ totals.sdk
		+ totals.dependency
		+ totals.injected_external
		+ totals.dynamic
		+ totals.blocked;
	totals.weak_or_unexplained = totals.candidate + totals.unknown_external + totals.unresolved;
	debug_assert_eq!(
		totals.references,
		totals.explained + totals.unknown_external + totals.unresolved,
		"resolution audit categories must partition every reference"
	);

	let mut clusters: Vec<AuditCluster> = clusters
		.into_iter()
		.map(|(pattern, (count, samples))| AuditCluster {
			id: pattern_id(&pattern),
			pattern,
			count,
			samples,
		})
		.collect();
	clusters.sort_by_key(|cluster| std::cmp::Reverse(cluster.count));
	clusters.truncate(options.cluster_limit);

	let mut zones: Vec<AuditZone> = zones
		.into_iter()
		.map(|(zone, (unresolved, patterns))| AuditZone {
			zone,
			unresolved,
			dominant_pattern: patterns
				.into_iter()
				.max_by_key(|(_, count)| *count)
				.map(|(label, _)| label)
				.unwrap_or_default(),
		})
		.collect();
	zones.sort_by_key(|zone| std::cmp::Reverse(zone.unresolved));
	zones.truncate(options.zone_limit);

	ResolutionAudit {
		totals,
		clusters,
		zones,
	}
}

fn classify_reference(
	lookups: &AuditLookups<'_>,
	reference: &ReferenceRecord,
	totals: &mut AuditTotals,
) -> Option<AuditClassification> {
	if let Some((evidence, target)) = lookups.resolved.get(&reference.id) {
		if !lookups.symbol_identities.contains_key(target) {
			totals.unresolved += 1;
			return Some(AuditClassification {
				status: "unresolved",
				reason: "dangling_binding",
				evidence: "linkage",
				scope: "unknown",
				candidate_targets: Vec::new(),
			});
		}
		if *evidence != crate::snapshot::ResolutionEvidence::NameMatch {
			totals.unique += 1;
			return None;
		}
		totals.candidate += 1;
		totals.name_match_candidate += 1;
		return Some(AuditClassification {
			status: "candidate",
			reason: "weak_name_match",
			evidence: evidence.as_str(),
			scope: "unknown",
			candidate_targets: Vec::new(),
		});
	}
	if let Some(candidate) = lookups.candidates.get(&reference.id) {
		totals.candidate += 1;
		if candidate.reason == crate::snapshot::CandidateReason::WeakNameMatch {
			totals.name_match_candidate += 1;
		}
		return Some(AuditClassification {
			status: "candidate",
			reason: candidate.reason.as_str(),
			evidence: candidate.evidence.as_str(),
			scope: candidate.scope.as_str(),
			candidate_targets: candidate_identities(&candidate.targets, &lookups.symbol_identities),
		});
	}
	if let Some(origin) = lookups.external.get(&reference.id) {
		totals.external += 1;
		match origin {
			crate::snapshot::ExternalReferenceOrigin::Sdk => {
				totals.sdk += 1;
				return None;
			}
			crate::snapshot::ExternalReferenceOrigin::Dependency => {
				totals.dependency += 1;
				return None;
			}
			crate::snapshot::ExternalReferenceOrigin::Injected => {
				totals.injected_external += 1;
				return None;
			}
			crate::snapshot::ExternalReferenceOrigin::UnknownExternal => {
				totals.unknown_external += 1;
				return Some(AuditClassification {
					status: "unknown_external",
					reason: origin.label(),
					evidence: "extractor",
					scope: "external",
					candidate_targets: Vec::new(),
				});
			}
		}
	}
	if let Some(dynamic) = lookups.dynamic.get(&reference.id) {
		totals.dynamic += 1;
		return Some(AuditClassification {
			status: "dynamic",
			reason: dynamic.reason.as_str(),
			evidence: "runtime",
			scope: "runtime",
			candidate_targets: candidate_identities(
				&dynamic.candidates,
				&lookups.symbol_identities,
			),
		});
	}
	if let Some(reason) = lookups.blocked.get(&reference.id) {
		totals.blocked += 1;
		return Some(AuditClassification {
			status: "blocked",
			reason,
			evidence: "policy",
			scope: "policy",
			candidate_targets: Vec::new(),
		});
	}
	totals.unresolved += 1;
	Some(AuditClassification {
		status: "unresolved",
		reason: lookups
			.unresolved
			.get(&reference.id)
			.copied()
			.unwrap_or("missing_decision"),
		evidence: "",
		scope: "",
		candidate_targets: Vec::new(),
	})
}

pub fn pattern_id(pattern: &AuditPattern) -> String {
	let mut hash = 0xcbf29ce484222325u64;
	for byte in pattern_label(pattern).bytes() {
		hash ^= u64::from(byte);
		hash = hash.wrapping_mul(0x100000001b3);
	}
	format!("resolution-{hash:016x}")
}

pub fn pattern_label(pattern: &AuditPattern) -> String {
	let mut label = format!("{} {}/{}", pattern.status, pattern.confidence, pattern.kind);
	if !pattern.reason.is_empty() {
		label.push_str(&format!(" reason:{}", pattern.reason));
	}
	if !pattern.evidence.is_empty() {
		label.push_str(&format!(" evidence:{}", pattern.evidence));
	}
	if !pattern.receiver.is_empty() {
		label.push_str(&format!(" recv:{}", pattern.receiver));
	}
	if !pattern.target_shape.is_empty() {
		label.push_str(&format!(" shape:{}", pattern.target_shape));
	}
	if !pattern.target_head.is_empty() {
		label.push_str(&format!(" head:{}", pattern.target_head));
	}
	if !pattern.srcset.is_empty() {
		label.push_str(&format!(" srcset:{}", pattern.srcset));
	}
	label
}

fn pattern_for(
	status: &str,
	reason: &str,
	evidence: &str,
	reference: &ReferenceRecord,
	source: &str,
) -> AuditPattern {
	let target = reference.target_identity.as_ref();
	AuditPattern {
		status: status.to_string(),
		reason: reason.to_string(),
		evidence: evidence.to_string(),
		confidence: reference.confidence.clone().unwrap_or_default(),
		kind: reference.kind.clone(),
		receiver: receiver_class(reference).to_string(),
		target_shape: target_shape(target),
		target_head: target_head(target, source),
		srcset: segment_value(target, "srcset:"),
	}
}

fn sample_for(
	reference: &ReferenceRecord,
	source: &str,
	file: &str,
	source_text: &str,
	classification: AuditClassification,
) -> AuditSample {
	AuditSample {
		file: file.to_string(),
		line_range: reference.line_range,
		snippet: source_excerpt(source_text, reference.line_range),
		source: identity_tail(source, 4),
		call_name: reference.call_name.clone().unwrap_or_default(),
		receiver: reference.receiver.clone().unwrap_or_default(),
		target: identity_tail(reference.target_identity.as_ref(), 5),
		evidence: classification.evidence.to_string(),
		constraints: sample_constraints(
			reference,
			classification.reason,
			classification.evidence,
			classification.scope,
		),
		candidates: classification.candidate_targets,
	}
}

fn source_excerpt(source: &str, line_range: Option<(u32, u32)>) -> String {
	let Some((start, end)) = line_range else {
		return String::new();
	};
	let line_count = end.saturating_sub(start).saturating_add(1).min(3) as usize;
	let excerpt = source
		.lines()
		.skip(start.saturating_sub(1) as usize)
		.take(line_count)
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.collect::<Vec<_>>()
		.join(" ");
	excerpt.chars().take(240).collect()
}

fn sample_constraints(
	reference: &ReferenceRecord,
	reason: &str,
	evidence: &str,
	scope: &str,
) -> Vec<String> {
	let mut constraints = vec![format!("kind:{}", reference.kind)];
	for (label, value) in [
		("reason", Some(reason)),
		("evidence", Some(evidence)),
		("scope", Some(scope)),
		("confidence", reference.confidence.as_deref()),
	] {
		if let Some(value) = value.filter(|value| !value.is_empty()) {
			constraints.push(format!("{label}:{value}"));
		}
	}
	if let Some(arity) = reference.call_arity {
		constraints.push(format!("arity:{arity}"));
	}
	constraints
}

fn candidate_identities(
	candidates: &[crate::snapshot::SymbolId],
	symbols: &HashMap<crate::snapshot::SymbolId, &str>,
) -> Vec<String> {
	candidates
		.iter()
		.filter_map(|candidate| symbols.get(candidate).copied())
		.take(8)
		.map(|identity| identity_tail(identity, 5))
		.collect()
}

fn receiver_class(reference: &ReferenceRecord) -> &'static str {
	match reference.receiver.as_deref() {
		None | Some("") => "",
		Some("call") => "call",
		Some("self" | "cls" | "this") => "self",
		Some(_) => "named",
	}
}

// Collapsed chain of segment kinds, consecutive repeats folded with `+`:
// `srcset/lang/package+/module/path/method` reads as a target shape.
fn target_shape(target: &str) -> String {
	let mut kinds: Vec<&str> = Vec::new();
	for segment in target.split('/') {
		let Some((kind, _)) = segment.split_once(':') else {
			continue;
		};
		if kind.contains('+') || kind.is_empty() {
			continue;
		}
		kinds.push(kind);
	}
	let mut collapsed: Vec<String> = Vec::new();
	for kind in kinds {
		match collapsed.last_mut() {
			Some(last) if last.trim_end_matches('+') == kind => {
				if !last.ends_with('+') {
					last.push('+');
				}
			}
			_ => collapsed.push(kind.to_string()),
		}
	}
	collapsed.join("/")
}

fn target_head(target: &str, source: &str) -> String {
	if let Some(root) = target
		.split('/')
		.find_map(|segment| segment.strip_prefix("external_pkg:"))
	{
		return format!("external_pkg:{root}");
	}
	if let (Some(source_module), Some(target_module)) =
		(module_prefix(source), module_prefix(target))
		&& source_module == target_module
	{
		return "own_module".to_string();
	}
	String::new()
}

fn module_prefix(identity: &str) -> Option<&str> {
	let idx = identity.find("/module:")?;
	let rest = &identity[idx + 1..];
	let end = rest
		.find('/')
		.map(|i| idx + 1 + i)
		.unwrap_or(identity.len());
	Some(&identity[..end])
}

fn segment_value(identity: &str, prefix: &str) -> String {
	identity
		.split('/')
		.find_map(|segment| segment.strip_prefix(prefix))
		.unwrap_or_default()
		.to_string()
}

fn zone_of(source: &str) -> String {
	match module_prefix(source) {
		Some(module) => identity_tail(module, 4),
		None => identity_tail(source, 3),
	}
}

fn identity_tail(identity: &str, segments: usize) -> String {
	let parts: Vec<&str> = identity
		.split('/')
		.filter(|part| !part.is_empty())
		.collect();
	let start = parts.len().saturating_sub(segments);
	parts[start..].join("/")
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use super::*;
	use crate::snapshot::{
		CandidateReason, CandidateReference, CandidateScope, ChangeOverlay, CodeIndex,
		DynamicReason, DynamicReference, ExternalReference, ExternalReferenceOrigin, LinkageEdge,
		LinkageReadIndexHandle, LinkageSnapshot, ReferenceId, ResourceGeneration, SourceCatalog,
		SourceFileRecord, SourceId, SymbolId, SymbolRecord, UnresolvedReason, UnresolvedReference,
		WorkspaceSnapshot, WorkspaceTimings,
	};

	#[test]
	fn dangling_resolved_binding_is_counted_unresolved_not_unique() {
		let generation = ResourceGeneration::new(1);
		let source = SourceId::at(0);
		let source_symbol = SymbolId::at(0, 0);
		let missing_target = SymbolId::at(9, 9);
		let mut source_record = SymbolRecord::new(source_symbol, source, "run", "function");
		source_record.identity =
			Arc::from("code+moniker://./lang:python/module:sample/function:run");
		let reference = ReferenceRecord::new(
			ReferenceId::at(0, 0),
			source,
			source_symbol,
			"code+moniker://./lang:python/module:sample/method:work",
			"method_call",
			Some((10, 10)),
		);
		let resolved = vec![LinkageEdge::new(ReferenceId::at(0, 0), missing_target)];
		let linkage = LinkageSnapshot {
			generation,
			index_generation: generation,
			resolved_refs: 1,
			candidate_refs: 0,
			external_refs: 0,
			dynamic_refs: 0,
			blocked_refs: 0,
			manifest_blocked_refs: 0,
			unresolved_refs: 0,
			ambiguous_refs: 0,
			read_index: LinkageReadIndexHandle::from_edges(&resolved),
			resolved,
			candidates: Vec::new(),
			external: Vec::new(),
			dynamic: Vec::new(),
			blocked: Vec::new(),
			manifest_blocked: Vec::new(),
			unresolved: Vec::new(),
		};
		let mut index = CodeIndex::with_references(
			generation,
			generation,
			vec![source_record],
			vec![reference],
		);
		index.sources.push(SourceFileRecord {
			id: source,
			uri: "file://sample.py".to_string(),
			source_root: 0,
			path: "sample.py".to_string(),
			rel_path: "src/sample.py".to_string(),
			anchor: "sample.py".to_string(),
			language: "python".to_string(),
			text: (0..20).map(|_| "value.work()\n").collect(),
		});
		let snapshot = WorkspaceSnapshot {
			generation,
			catalog: SourceCatalog::new(generation, Vec::new()),
			index,
			linkage,
			changes: ChangeOverlay::new(generation, generation, generation, Vec::new()),
			timings: WorkspaceTimings::default(),
		};

		let audit = resolution_audit(&snapshot, "lang:python", AuditOptions::default());

		assert_eq!(
			audit.totals.unique, 0,
			"a binding to a symbol absent from the index must not count unique"
		);
		assert_eq!(audit.totals.unresolved, 1, "{:?}", audit.totals);
		assert!(
			audit
				.clusters
				.iter()
				.any(|cluster| cluster.pattern.reason == "dangling_binding"),
			"the dangling binding must cluster under its own reason: {:?}",
			audit.clusters
		);
	}

	#[test]
	fn totals_partition_unique_candidate_external_dynamic_blocked_and_unresolved() {
		let generation = ResourceGeneration::new(1);
		let source = SourceId::at(0);
		let source_symbol = SymbolId::at(0, 0);
		let candidate_target = SymbolId::at(0, 1);
		let mut source_record = SymbolRecord::new(source_symbol, source, "run", "function");
		source_record.identity =
			Arc::from("code+moniker://./lang:python/module:sample/function:run");
		let mut target_record = SymbolRecord::new(candidate_target, source, "Target", "class");
		target_record.identity =
			Arc::from("code+moniker://./lang:python/module:sample/class:Target");
		let references = (0..9)
			.map(|idx| {
				ReferenceRecord::new(
					ReferenceId::at(0, idx),
					source,
					source_symbol,
					"code+moniker://./lang:python/module:sample/method:work",
					"method_call",
					Some((10 + idx as u32, 10 + idx as u32)),
				)
				.with_metadata(
					Some("resolved".to_string()),
					Some("value".to_string()),
					None,
				)
			})
			.collect::<Vec<_>>();
		let linkage = fixture_linkage(generation, candidate_target);
		let mut index = CodeIndex::with_references(
			generation,
			generation,
			vec![source_record, target_record],
			references,
		);
		index.sources.push(SourceFileRecord {
			id: source,
			uri: "file://sample.py".to_string(),
			source_root: 0,
			path: "sample.py".to_string(),
			rel_path: "src/sample.py".to_string(),
			anchor: "sample.py".to_string(),
			language: "python".to_string(),
			text: (0..20).map(|_| "value.work()\n").collect(),
		});
		let snapshot = WorkspaceSnapshot {
			generation,
			catalog: SourceCatalog::new(generation, Vec::new()),
			index,
			linkage,
			changes: ChangeOverlay::new(generation, generation, generation, Vec::new()),
			timings: WorkspaceTimings::default(),
		};

		let audit = resolution_audit(&snapshot, "lang:python", AuditOptions::default());

		assert_eq!(audit.totals.references, 9);
		assert_eq!(audit.totals.unique, 1);
		assert_eq!(audit.totals.candidate, 1);
		assert_eq!(audit.totals.external, 4);
		assert_eq!(audit.totals.sdk, 1);
		assert_eq!(audit.totals.dependency, 1);
		assert_eq!(audit.totals.injected_external, 1);
		assert_eq!(audit.totals.unknown_external, 1);
		assert_eq!(audit.totals.dynamic, 1);
		assert_eq!(audit.totals.blocked, 1);
		assert_eq!(audit.totals.unresolved, 1);
		assert_eq!(audit.totals.explained, 7);
		assert_eq!(audit.totals.weak_or_unexplained, 3);
		assert_audit_clusters(&audit);
		let candidate_cluster = audit
			.clusters
			.iter()
			.find(|cluster| cluster.pattern.status == "candidate")
			.expect("candidate cluster");
		let drill_down = resolution_audit(
			&snapshot,
			"lang:python",
			AuditOptions {
				cluster: Some(candidate_cluster.id.clone()),
				sample_offset: 0,
				sample_limit: 1,
				..AuditOptions::default()
			},
		);
		assert_eq!(drill_down.clusters.len(), 1);
		assert_eq!(drill_down.clusters[0].id, candidate_cluster.id);
		assert_eq!(drill_down.clusters[0].samples.len(), 1);
	}

	fn fixture_linkage(
		generation: ResourceGeneration,
		candidate_target: SymbolId,
	) -> LinkageSnapshot {
		let resolved = vec![LinkageEdge::new(ReferenceId::at(0, 0), candidate_target)];
		let manifest_blocked = UnresolvedReference::new(
			ReferenceId::at(0, 4),
			"code+moniker://./lang:python/module:sample/method:work",
			UnresolvedReason::ManifestBlocked,
		);
		LinkageSnapshot {
			generation,
			index_generation: generation,
			resolved_refs: 1,
			candidate_refs: 1,
			external_refs: 4,
			dynamic_refs: 1,
			blocked_refs: 1,
			manifest_blocked_refs: 1,
			unresolved_refs: 1,
			ambiguous_refs: 1,
			read_index: LinkageReadIndexHandle::from_edges(&resolved),
			resolved,
			candidates: vec![CandidateReference::new(
				ReferenceId::at(0, 1),
				vec![candidate_target],
				CandidateReason::WeakNameMatch,
				CandidateScope::Global,
				crate::snapshot::ResolutionEvidence::NameMatch,
			)],
			external: vec![
				ExternalReference::new(
					ReferenceId::at(0, 2),
					"code+moniker://./external_pkg:sample/path:work",
					ExternalReferenceOrigin::Dependency,
				),
				ExternalReference::new(
					ReferenceId::at(0, 6),
					"code+moniker://./sdk:python/path:builtins/path:print",
					ExternalReferenceOrigin::Sdk,
				),
				ExternalReference::new(
					ReferenceId::at(0, 7),
					"code+moniker://./external_pkg:generated/path:work",
					ExternalReferenceOrigin::Injected,
				),
				ExternalReference::new(
					ReferenceId::at(0, 8),
					"code+moniker://./external_pkg:unknown/path:work",
					ExternalReferenceOrigin::UnknownExternal,
				),
			],
			dynamic: vec![DynamicReference::new(
				ReferenceId::at(0, 3),
				"code+moniker://./lang:python/module:sample/method:work",
				DynamicReason::DynamicAttribute,
				Vec::new(),
			)],
			blocked: vec![manifest_blocked.clone()],
			manifest_blocked: vec![manifest_blocked],
			unresolved: vec![UnresolvedReference::new(
				ReferenceId::at(0, 5),
				"code+moniker://./lang:python/module:sample/method:work",
				UnresolvedReason::NoCandidate,
			)],
		}
	}

	fn assert_audit_clusters(audit: &ResolutionAudit) {
		assert!(audit.clusters.iter().any(|cluster| {
			cluster.pattern.status == "candidate"
				&& cluster.pattern.reason == "weak_name_match"
				&& cluster.pattern.evidence == "name_match"
				&& cluster.samples.iter().any(|sample| {
					sample.file == "src/sample.py"
						&& sample.line_range == Some((11, 11))
						&& sample.snippet == "value.work()"
						&& sample.constraints.contains(&"scope:global".to_string())
						&& sample
							.candidates
							.iter()
							.any(|candidate| candidate.ends_with("class:Target"))
				})
		}));
		assert!(audit.clusters.iter().any(|cluster| {
			cluster.pattern.status == "dynamic" && cluster.pattern.reason == "dynamic_attribute"
		}));
		assert!(audit.clusters.iter().any(|cluster| {
			cluster.pattern.status == "blocked"
				&& cluster.pattern.reason == "manifest_blocked"
				&& cluster.pattern.evidence == "policy"
		}));
		assert!(audit.clusters.iter().any(|cluster| {
			cluster.pattern.status == "unknown_external"
				&& cluster.pattern.reason == "unknown_external"
				&& cluster.pattern.evidence == "extractor"
		}));
	}
}
