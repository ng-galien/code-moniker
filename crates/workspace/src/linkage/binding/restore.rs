use std::collections::BTreeMap;

use code_moniker_core::core::uri::{UriConfig, from_uri};
use rustc_hash::FxHashMap;

use super::{
	BlockReason, ReferenceLinkageDecision, ResolutionDecision, ResolutionScope, UnknownReason,
};
use crate::linkage::catalog::{
	CandidateCatalog, ReferenceOrdinal, SymbolOrdinalCatalog, SymbolSet,
};
use crate::snapshot::{
	CandidateScope, LinkageSnapshot, RecordTable, ReferenceId, ReferenceRecord, SymbolId,
};
use crate::source::CodeIndexMaterial;

pub(super) fn decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	references: &RecordTable<ReferenceRecord>,
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog,
) -> Vec<ReferenceLinkageDecision> {
	let reference_indexes = super::indexes::reference_indexes(references);
	let mut decisions = Vec::new();
	decisions.extend(resolved_decisions_from_snapshot(
		snapshot,
		&reference_indexes,
		candidates.symbols(),
	));
	decisions.extend(candidate_decisions_from_snapshot(
		snapshot,
		&reference_indexes,
		candidates.symbols(),
	));
	decisions.extend(dynamic_decisions_from_snapshot(
		snapshot,
		&reference_indexes,
		candidates.symbols(),
	));
	decisions.extend(external_decisions_from_snapshot(
		snapshot,
		&reference_indexes,
		material,
	));
	let blocked = if snapshot.blocked.is_empty() {
		&snapshot.manifest_blocked
	} else {
		&snapshot.blocked
	};
	decisions.extend(blocked.iter().filter_map(|blocked| {
		let reference_idx = reference_indexes.get(&blocked.reference)?.index();
		let reason = BlockReason::from_unresolved_reason(blocked.reason)?;
		Some(ReferenceLinkageDecision::blocked(
			reason,
			reference_idx,
			blocked.reference,
		))
	}));
	decisions.extend(snapshot.unresolved.iter().filter_map(|unresolved| {
		let reference_idx = reference_indexes.get(&unresolved.reference)?.index();
		let reason = UnknownReason::from_unresolved_reason(unresolved.reason)?;
		Some(ReferenceLinkageDecision::unknown(
			reason,
			reference_idx,
			references[reference_idx].id,
		))
	}));
	decisions
}

fn resolved_decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
	symbols: &SymbolOrdinalCatalog,
) -> Vec<ReferenceLinkageDecision> {
	let mut targets_by_reference = BTreeMap::new();
	for edge in &snapshot.resolved {
		let entry = targets_by_reference
			.entry(edge.reference)
			.or_insert_with(|| (SymbolSet::new(), edge.evidence, false));
		match symbols.ordinal(&edge.target) {
			Some(target) => {
				entry.0.insert(target);
			}
			None => entry.2 = true,
		}
	}
	targets_by_reference
		.into_iter()
		.filter_map(|(reference, (targets, evidence, missing_target))| {
			reference_indexes.get(&reference).map(|reference_idx| {
				let reference_idx = reference_idx.index();
				if missing_target {
					ReferenceLinkageDecision::unknown(
						UnknownReason::NoCandidate,
						reference_idx,
						reference,
					)
				} else {
					ReferenceLinkageDecision::resolved(ResolutionDecision::new(
						ResolutionScope::Global,
						evidence,
						reference,
						reference_idx,
						targets,
					))
				}
			})
		})
		.collect()
}

fn candidate_decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
	symbols: &SymbolOrdinalCatalog,
) -> Vec<ReferenceLinkageDecision> {
	snapshot
		.candidates
		.iter()
		.filter_map(|candidate| {
			let reference_idx = reference_indexes.get(&candidate.reference)?.index();
			let targets = candidate
				.targets
				.iter()
				.filter_map(|target| symbols.ordinal(target))
				.collect::<SymbolSet>();
			if targets.is_empty() || targets.len() != candidate.targets.len() {
				Some(ReferenceLinkageDecision::unknown(
					UnknownReason::NoCandidate,
					reference_idx,
					candidate.reference,
				))
			} else {
				Some(ReferenceLinkageDecision::candidate(
					candidate.reason,
					ResolutionDecision::new(
						resolution_scope(candidate.scope),
						candidate.evidence,
						candidate.reference,
						reference_idx,
						targets,
					),
				))
			}
		})
		.collect()
}

fn dynamic_decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
	symbols: &SymbolOrdinalCatalog,
) -> Vec<ReferenceLinkageDecision> {
	snapshot
		.dynamic
		.iter()
		.filter_map(|dynamic| {
			let reference_idx = reference_indexes.get(&dynamic.reference)?.index();
			let candidates = advisory_dynamic_candidates(&dynamic.candidates, symbols);
			Some(ReferenceLinkageDecision::dynamic(
				dynamic.reason,
				reference_idx,
				dynamic.reference,
				candidates,
			))
		})
		.collect()
}

/// Restores dynamic candidate hints without treating them as an exhaustive target set.
/// The dynamic classification remains valid when stale hints disappear and stays outside
/// the unique graph.
fn advisory_dynamic_candidates(
	candidates: &[SymbolId],
	symbols: &SymbolOrdinalCatalog,
) -> SymbolSet {
	candidates
		.iter()
		.filter_map(|target| symbols.ordinal(target))
		.collect()
}

fn resolution_scope(scope: CandidateScope) -> ResolutionScope {
	match scope {
		CandidateScope::Local => ResolutionScope::Local,
		CandidateScope::Global => ResolutionScope::Global,
		CandidateScope::Builtin => ResolutionScope::Builtin,
		CandidateScope::Injected => ResolutionScope::Injected,
		CandidateScope::Unknown => ResolutionScope::Unknown,
	}
}

fn external_decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
	material: &CodeIndexMaterial,
) -> Vec<ReferenceLinkageDecision> {
	snapshot
		.external
		.iter()
		.filter_map(|external| {
			let reference_idx = reference_indexes.get(&external.reference)?.index();
			let target = from_uri(
				external.target_identity.as_ref(),
				&UriConfig {
					scheme: material.identity.scheme(),
				},
			)
			.ok();
			Some(match target {
				Some(target) => ReferenceLinkageDecision::external_target(
					external.origin,
					reference_idx,
					external.reference,
					target,
				),
				None => ReferenceLinkageDecision::external(
					external.origin,
					reference_idx,
					external.reference,
				),
			})
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::snapshot::{
		CandidateReason, CandidateReference, DynamicReason, DynamicReference, ResolutionEvidence,
		ResourceGeneration,
	};

	#[test]
	fn candidate_reconstruction_invalidates_a_partially_missing_target_set() {
		let reference = ReferenceId::at(0, 0);
		let present = SymbolId::at(0, 0);
		let missing = SymbolId::at(1, 0);
		let mut snapshot =
			LinkageSnapshot::new(ResourceGeneration::new(1), ResourceGeneration::new(1), 0, 0);
		snapshot.candidates.push(CandidateReference::new(
			reference,
			vec![present, missing],
			CandidateReason::MultipleTargets,
			CandidateScope::Global,
			ResolutionEvidence::GlobalBinding,
		));
		let mut reference_indexes = FxHashMap::default();
		reference_indexes.insert(reference, ReferenceOrdinal::from_index(0));
		let mut symbols = SymbolOrdinalCatalog::default();
		symbols.push(present);

		let decisions = candidate_decisions_from_snapshot(&snapshot, &reference_indexes, &symbols);

		assert!(matches!(
			decisions.as_slice(),
			[ReferenceLinkageDecision::Unknown {
				reason: UnknownReason::NoCandidate,
				..
			}]
		));
	}

	#[test]
	fn unknown_candidate_scope_round_trips_without_becoming_global() {
		assert_eq!(
			resolution_scope(CandidateScope::Unknown),
			ResolutionScope::Unknown
		);
	}

	#[test]
	fn dynamic_reconstruction_keeps_the_classification_when_advisory_targets_disappear() {
		let reference = ReferenceId::at(0, 0);
		let present = SymbolId::at(0, 0);
		let missing = SymbolId::at(1, 0);
		let mut snapshot =
			LinkageSnapshot::new(ResourceGeneration::new(1), ResourceGeneration::new(1), 0, 0);
		snapshot.dynamic.push(DynamicReference::new(
			reference,
			"method:runtime_value",
			DynamicReason::DynamicAttribute,
			vec![present, missing],
		));
		let mut reference_indexes = FxHashMap::default();
		reference_indexes.insert(reference, ReferenceOrdinal::from_index(0));
		let mut symbols = SymbolOrdinalCatalog::default();
		symbols.push(present);

		let decisions = dynamic_decisions_from_snapshot(&snapshot, &reference_indexes, &symbols);

		assert!(matches!(
			decisions.as_slice(),
			[ReferenceLinkageDecision::Dynamic {
				reason: DynamicReason::DynamicAttribute,
				candidates,
				..
			}] if candidates.len() == 1
		));
	}
}
