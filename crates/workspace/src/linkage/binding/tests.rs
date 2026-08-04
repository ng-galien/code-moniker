use super::*;
use crate::linkage::catalog::{SymbolOrdinalCatalog, SymbolSet};
use crate::snapshot::{
	CandidateReason, DynamicReason, LinkageSnapshot, RecordTable, ReferenceId, ReferenceRecord,
	ResolutionEvidence, ResourceGeneration, SourceId, SymbolId, UnresolvedReason,
};
use crate::source::LocalIdentityResolver;

fn projected_resolution(confidence: &str, target_count: usize) -> LinkageSnapshot {
	let source = SourceId::at(0);
	let reference_id = ReferenceId::at(0, 0);
	let reference = ReferenceRecord::new(
		reference_id,
		source,
		SymbolId::at(0, 0),
		"code+moniker://./lang:python/module:sample/method:run",
		"method_call",
		None,
	)
	.with_metadata(Some(confidence.to_string()), None, None);
	let references = RecordTable::from_records(vec![reference]);
	let mut symbols = SymbolOrdinalCatalog::default();
	let targets: SymbolSet = (0..target_count)
		.map(|idx| symbols.push(SymbolId::at(0, idx + 1)))
		.collect();
	let decisions = vec![ReferenceLinkageDecision::resolved(ResolutionDecision::new(
		ResolutionScope::Global,
		if confidence == "name_match" {
			ResolutionEvidence::NameMatch
		} else {
			ResolutionEvidence::ExactBinding
		},
		reference_id,
		0,
		targets,
	))];

	project_decisions(
		&decisions,
		&references,
		&LocalIdentityResolver::default(),
		&symbols,
	)
	.into_snapshot(ResourceGeneration::new(1), ResourceGeneration::new(1))
}

#[test]
fn multiple_targets_are_candidates_not_graph_edges() {
	let snapshot = projected_resolution("resolved", 2);

	assert_eq!(snapshot.resolved_refs, 0);
	assert!(snapshot.resolved.is_empty());
	assert_eq!(snapshot.candidate_refs, 1);
	assert_eq!(snapshot.candidates.len(), 1);
	assert_eq!(snapshot.candidates[0].targets.len(), 2);
	assert_eq!(
		snapshot.candidates[0].reason,
		CandidateReason::MultipleTargets
	);
}

#[test]
fn weak_single_name_match_is_a_candidate_not_a_unique_target() {
	let snapshot = projected_resolution("name_match", 1);

	assert_eq!(snapshot.resolved_refs, 0);
	assert!(snapshot.resolved.is_empty());
	assert_eq!(snapshot.candidate_refs, 1);
	assert_eq!(snapshot.candidates.len(), 1);
	assert_eq!(snapshot.candidates[0].targets.len(), 1);
	assert_eq!(
		snapshot.candidates[0].reason,
		CandidateReason::WeakNameMatch
	);
}

#[test]
fn strong_single_target_remains_a_unique_graph_edge() {
	let snapshot = projected_resolution("resolved", 1);

	assert_eq!(snapshot.resolved_refs, 1);
	assert_eq!(snapshot.resolved.len(), 1);
	assert_eq!(snapshot.candidate_refs, 0);
	assert!(snapshot.candidates.is_empty());
}

#[test]
fn dynamic_decisions_are_explained_without_graph_edges() {
	let source = SourceId::at(0);
	let reference_id = ReferenceId::at(0, 0);
	let reference = ReferenceRecord::new(
		reference_id,
		source,
		SymbolId::at(0, 0),
		"code+moniker://./lang:python/module:sample/method:run",
		"method_call",
		None,
	);
	let references = RecordTable::from_records(vec![reference]);
	let decisions = vec![ReferenceLinkageDecision::dynamic(
		DynamicReason::DynamicAttribute,
		0,
		reference_id,
		SymbolSet::new(),
	)];

	let snapshot = project_decisions(
		&decisions,
		&references,
		&LocalIdentityResolver::default(),
		&SymbolOrdinalCatalog::default(),
	)
	.into_snapshot(ResourceGeneration::new(1), ResourceGeneration::new(1));

	assert_eq!(snapshot.resolved_refs, 0);
	assert!(snapshot.resolved.is_empty());
	assert_eq!(snapshot.dynamic_refs, 1);
	assert_eq!(snapshot.dynamic.len(), 1);
	assert_eq!(snapshot.dynamic[0].reason, DynamicReason::DynamicAttribute);
	assert_eq!(snapshot.unresolved_refs, 0);
}

#[test]
fn non_manifest_blocks_remain_blocked_not_unresolved() {
	let source = SourceId::at(0);
	let reference_id = ReferenceId::at(0, 0);
	let reference = ReferenceRecord::new(
		reference_id,
		source,
		SymbolId::at(0, 0),
		"code+moniker://./lang:python/module:sample/method:run",
		"method_call",
		None,
	);
	let references = RecordTable::from_records(vec![reference]);
	let decisions = vec![ReferenceLinkageDecision::blocked(
		BlockReason::Visibility,
		0,
		reference_id,
	)];

	let snapshot = project_decisions(
		&decisions,
		&references,
		&LocalIdentityResolver::default(),
		&SymbolOrdinalCatalog::default(),
	)
	.into_snapshot(ResourceGeneration::new(1), ResourceGeneration::new(1));

	assert_eq!(snapshot.blocked_refs, 1);
	assert_eq!(snapshot.blocked.len(), 1);
	assert_eq!(snapshot.blocked[0].reason, UnresolvedReason::Visibility);
	assert_eq!(snapshot.manifest_blocked_refs, 0);
	assert!(snapshot.manifest_blocked.is_empty());
	assert_eq!(snapshot.unresolved_refs, 0);
}

#[test]
fn external_blocked_and_unknown_decisions_reach_their_snapshot_collections() {
	let source = SourceId::at(0);
	let references = RecordTable::from_records(
		(0..3)
			.map(|ordinal| {
				ReferenceRecord::new(
					ReferenceId::at(0, ordinal),
					source,
					SymbolId::at(0, ordinal),
					format!("code+moniker://./lang:java/type:Target{ordinal}"),
					"type_use",
					None,
				)
			})
			.collect(),
	);
	let decisions = vec![
		ReferenceLinkageDecision::external(ExternalOrigin::Dependency, 0, ReferenceId::at(0, 0)),
		ReferenceLinkageDecision::manifest_blocked(1, ReferenceId::at(0, 1)),
		ReferenceLinkageDecision::unknown(UnknownReason::MissingQuery, 2, ReferenceId::at(0, 2)),
	];

	let snapshot = project_decisions(
		&decisions,
		&references,
		&LocalIdentityResolver::default(),
		&SymbolOrdinalCatalog::default(),
	)
	.into_snapshot(ResourceGeneration::new(1), ResourceGeneration::new(1));

	assert_eq!(snapshot.external_refs, 1);
	assert_eq!(snapshot.external[0].origin, ExternalOrigin::Dependency);
	assert_eq!(snapshot.blocked_refs, 1);
	assert_eq!(snapshot.manifest_blocked_refs, 1);
	assert_eq!(
		snapshot.manifest_blocked[0].reason,
		UnresolvedReason::ManifestBlocked
	);
	assert_eq!(snapshot.unresolved_refs, 1);
	assert_eq!(
		snapshot.unresolved[0].reason,
		UnresolvedReason::MissingQuery
	);
}

#[test]
fn unresolved_reasons_round_trip_through_internal_decisions() {
	for reason in [
		UnresolvedReason::MissingQuery,
		UnresolvedReason::NoCandidate,
		UnresolvedReason::Ambiguous,
		UnresolvedReason::UnsupportedLanguageRule,
		UnresolvedReason::IncompleteExtractorMetadata,
	] {
		let internal = UnknownReason::from_unresolved_reason(reason).expect("unknown reason");
		assert_eq!(internal.unresolved_reason(), reason);
	}
}
