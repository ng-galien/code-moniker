use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::snapshot::{ReferenceRecord, WorkspaceSnapshot};

// Embedded resolution audit: every unresolved reference (and every resolved
// name-match, the false-link candidates) is classified under a mechanical
// pattern key — the exact dimensions that drove the R4 diagnoses by hand.
// Labels are facts about the reference, never guesses about the cause.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolutionAudit {
	pub totals: AuditTotals,
	pub clusters: Vec<AuditCluster>,
	pub zones: Vec<AuditZone>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditTotals {
	pub references: usize,
	pub resolved: usize,
	pub external: usize,
	pub blocked: usize,
	pub unresolved: usize,
	pub name_match_resolved: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditCluster {
	pub pattern: AuditPattern,
	pub count: usize,
	pub samples: Vec<AuditSample>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct AuditPattern {
	pub status: String,
	pub confidence: String,
	pub kind: String,
	pub receiver: String,
	pub target_shape: String,
	pub target_head: String,
	pub srcset: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditSample {
	pub source: String,
	pub call_name: String,
	pub receiver: String,
	pub target: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditZone {
	pub zone: String,
	pub unresolved: usize,
	pub dominant_pattern: String,
}

#[derive(Clone, Copy, Debug)]
pub struct AuditOptions {
	pub cluster_limit: usize,
	pub sample_limit: usize,
	pub zone_limit: usize,
}

impl Default for AuditOptions {
	fn default() -> Self {
		Self {
			cluster_limit: 20,
			sample_limit: 3,
			zone_limit: 10,
		}
	}
}

pub fn resolution_audit(
	snapshot: &WorkspaceSnapshot,
	prefix: &str,
	options: AuditOptions,
) -> ResolutionAudit {
	let sources: HashMap<_, _> = snapshot
		.index
		.symbols
		.iter()
		.map(|symbol| (symbol.id, symbol.identity.as_ref()))
		.collect();
	let unresolved: HashMap<_, _> = snapshot
		.linkage
		.unresolved
		.iter()
		.map(|item| (item.reference, ()))
		.collect();
	let blocked: HashMap<_, _> = snapshot
		.linkage
		.manifest_blocked
		.iter()
		.map(|item| (item.reference, ()))
		.collect();
	let external: HashMap<_, _> = snapshot
		.linkage
		.external
		.iter()
		.map(|item| (item.reference, ()))
		.collect();
	let resolved: HashMap<_, _> = snapshot
		.linkage
		.resolved
		.iter()
		.map(|edge| (edge.reference, ()))
		.collect();

	let mut totals = AuditTotals::default();
	let mut clusters: HashMap<AuditPattern, (usize, Vec<AuditSample>)> = HashMap::new();
	let mut zones: HashMap<String, (usize, HashMap<String, usize>)> = HashMap::new();

	for reference in snapshot.index.references.iter() {
		let source = sources
			.get(&reference.source_symbol)
			.copied()
			.unwrap_or_default();
		if !prefix.is_empty() && !source.contains(prefix) {
			continue;
		}
		totals.references += 1;
		let status = if resolved.contains_key(&reference.id) {
			totals.resolved += 1;
			if reference.confidence.as_deref() != Some("name_match") {
				continue;
			}
			totals.name_match_resolved += 1;
			"resolved_name_match"
		} else if external.contains_key(&reference.id) {
			totals.external += 1;
			continue;
		} else if blocked.contains_key(&reference.id) {
			totals.blocked += 1;
			continue;
		} else if unresolved.contains_key(&reference.id) {
			totals.unresolved += 1;
			"unresolved"
		} else {
			continue;
		};

		let pattern = pattern_for(status, reference, source);
		let entry = clusters.entry(pattern.clone()).or_default();
		entry.0 += 1;
		if entry.1.len() < options.sample_limit {
			entry.1.push(sample_for(reference, source));
		}
		if status == "unresolved" {
			let zone = zone_of(source);
			let slot = zones.entry(zone).or_default();
			slot.0 += 1;
			*slot.1.entry(pattern_label(&pattern)).or_default() += 1;
		}
	}

	let mut clusters: Vec<AuditCluster> = clusters
		.into_iter()
		.map(|(pattern, (count, samples))| AuditCluster {
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

pub fn pattern_label(pattern: &AuditPattern) -> String {
	let mut label = format!("{} {}/{}", pattern.status, pattern.confidence, pattern.kind);
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

fn pattern_for(status: &str, reference: &ReferenceRecord, source: &str) -> AuditPattern {
	let target = reference.target_identity.as_ref();
	AuditPattern {
		status: status.to_string(),
		confidence: reference.confidence.clone().unwrap_or_default(),
		kind: reference.kind.clone(),
		receiver: receiver_class(reference).to_string(),
		target_shape: target_shape(target),
		target_head: target_head(target, source),
		srcset: segment_value(target, "srcset:"),
	}
}

fn sample_for(reference: &ReferenceRecord, source: &str) -> AuditSample {
	AuditSample {
		source: identity_tail(source, 4),
		call_name: reference.call_name.clone().unwrap_or_default(),
		receiver: reference.receiver.clone().unwrap_or_default(),
		target: identity_tail(reference.target_identity.as_ref(), 5),
	}
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
