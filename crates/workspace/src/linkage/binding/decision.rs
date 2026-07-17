use crate::linkage::catalog::{SymbolOrdinalCatalog, SymbolSet};
use crate::snapshot::{
	CandidateReason, CandidateReference, CandidateScope, DynamicReason, DynamicReference,
	ExternalReference, LinkageEdge, LinkageSnapshot, RecordTable, ReferenceId, ReferenceRecord,
	ResolutionEvidence, ResourceGeneration, UnresolvedReason, UnresolvedReference,
};
use crate::source::LocalIdentityResolver;
use code_moniker_core::core::moniker::Moniker;
use std::sync::Arc;

pub(in crate::linkage) use crate::snapshot::ExternalReferenceOrigin as ExternalOrigin;

pub(in crate::linkage) fn project_decisions(
	decisions: &[ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	identity: &LocalIdentityResolver,
	symbols: &SymbolOrdinalCatalog,
) -> LinkageReportProjection {
	LinkageReportProjection::from_decisions(decisions, references, identity, symbols)
}

// This is a discriminated union of mutually exclusive outcomes, not a stateful
// class; LCOM cohesion across variant-specific constructors is not meaningful.
// code-moniker: ignore[rust.shape.type.smell-god-type-local-metrics]
#[derive(Clone)]
pub(in crate::linkage) enum ReferenceLinkageDecision {
	Unique {
		resolution: ResolutionDecision,
	},
	Candidate {
		reason: CandidateReason,
		resolution: ResolutionDecision,
	},
	External {
		origin: ExternalOrigin,
		reference: ReferenceId,
		reference_idx: usize,
		target: Option<Moniker>,
	},
	Blocked {
		reason: BlockReason,
		reference: ReferenceId,
		reference_idx: usize,
	},
	Dynamic {
		reason: DynamicReason,
		reference: ReferenceId,
		reference_idx: usize,
		candidates: SymbolSet,
	},
	Unknown {
		reason: UnknownReason,
		reference: ReferenceId,
		reference_idx: usize,
	},
}

#[derive(Clone)]
pub(in crate::linkage) struct ResolutionDecision {
	pub(in crate::linkage) scope: ResolutionScope,
	pub(in crate::linkage) evidence: ResolutionEvidence,
	pub(in crate::linkage) reference: ReferenceId,
	pub(in crate::linkage) reference_idx: usize,
	pub(in crate::linkage) targets: SymbolSet,
}

impl ResolutionDecision {
	pub(in crate::linkage) fn new(
		scope: ResolutionScope,
		evidence: ResolutionEvidence,
		reference: ReferenceId,
		reference_idx: usize,
		targets: SymbolSet,
	) -> Self {
		Self {
			scope,
			evidence,
			reference,
			reference_idx,
			targets,
		}
	}

	fn candidate_reason(&self) -> Option<CandidateReason> {
		if self.evidence == ResolutionEvidence::NameMatch {
			Some(CandidateReason::WeakNameMatch)
		} else if self.targets.len() > 1 {
			Some(CandidateReason::MultipleTargets)
		} else {
			None
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(in crate::linkage) enum ResolutionScope {
	Local,
	Global,
	Builtin,
	Injected,
	Unknown,
}

impl From<ResolutionScope> for CandidateScope {
	fn from(scope: ResolutionScope) -> Self {
		match scope {
			ResolutionScope::Local => Self::Local,
			ResolutionScope::Global => Self::Global,
			ResolutionScope::Builtin => Self::Builtin,
			ResolutionScope::Injected => Self::Injected,
			ResolutionScope::Unknown => Self::Unknown,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(in crate::linkage) enum BlockReason {
	ManifestPolicy,
	Visibility,
	LanguageBoundary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(in crate::linkage) enum UnknownReason {
	MissingQuery,
	NoCandidate,
	Ambiguous,
	UnsupportedLanguageRule,
	IncompleteExtractorMetadata,
}

impl BlockReason {
	fn unresolved_reason(&self) -> UnresolvedReason {
		match self {
			Self::ManifestPolicy => UnresolvedReason::ManifestBlocked,
			Self::Visibility => UnresolvedReason::Visibility,
			Self::LanguageBoundary => UnresolvedReason::LanguageBoundary,
		}
	}

	pub(in crate::linkage) fn from_unresolved_reason(reason: UnresolvedReason) -> Option<Self> {
		match reason {
			UnresolvedReason::ManifestBlocked => Some(Self::ManifestPolicy),
			UnresolvedReason::Visibility => Some(Self::Visibility),
			UnresolvedReason::LanguageBoundary => Some(Self::LanguageBoundary),
			_ => None,
		}
	}
}

impl UnknownReason {
	fn unresolved_reason(&self) -> UnresolvedReason {
		match self {
			Self::MissingQuery => UnresolvedReason::MissingQuery,
			Self::NoCandidate => UnresolvedReason::NoCandidate,
			Self::Ambiguous => UnresolvedReason::Ambiguous,
			Self::UnsupportedLanguageRule => UnresolvedReason::UnsupportedLanguageRule,
			Self::IncompleteExtractorMetadata => UnresolvedReason::IncompleteExtractorMetadata,
		}
	}

	pub(in crate::linkage) fn from_unresolved_reason(reason: UnresolvedReason) -> Option<Self> {
		match reason {
			UnresolvedReason::MissingQuery => Some(Self::MissingQuery),
			UnresolvedReason::NoCandidate => Some(Self::NoCandidate),
			UnresolvedReason::Ambiguous => Some(Self::Ambiguous),
			UnresolvedReason::UnsupportedLanguageRule => Some(Self::UnsupportedLanguageRule),
			UnresolvedReason::IncompleteExtractorMetadata => {
				Some(Self::IncompleteExtractorMetadata)
			}
			UnresolvedReason::ManifestBlocked
			| UnresolvedReason::Visibility
			| UnresolvedReason::LanguageBoundary => None,
		}
	}
}

impl ReferenceLinkageDecision {
	pub(in crate::linkage) fn resolved(resolution: ResolutionDecision) -> Self {
		if resolution.targets.is_empty() {
			return Self::unknown(
				UnknownReason::NoCandidate,
				resolution.reference_idx,
				resolution.reference,
			);
		}
		match resolution.candidate_reason() {
			Some(reason) => Self::candidate(reason, resolution),
			None => Self::Unique { resolution },
		}
	}

	pub(in crate::linkage) fn candidate(
		reason: CandidateReason,
		resolution: ResolutionDecision,
	) -> Self {
		Self::Candidate { reason, resolution }
	}

	#[allow(dead_code)]
	pub(in crate::linkage) fn dynamic(
		reason: DynamicReason,
		reference_idx: usize,
		reference: ReferenceId,
		candidates: SymbolSet,
	) -> Self {
		Self::Dynamic {
			reason,
			reference,
			reference_idx,
			candidates,
		}
	}

	pub(in crate::linkage) fn unknown(
		reason: UnknownReason,
		reference_idx: usize,
		reference: ReferenceId,
	) -> Self {
		Self::Unknown {
			reason,
			reference,
			reference_idx,
		}
	}

	pub(in crate::linkage) fn manifest_blocked(
		reference_idx: usize,
		reference: ReferenceId,
	) -> Self {
		Self::blocked(BlockReason::ManifestPolicy, reference_idx, reference)
	}

	pub(in crate::linkage) fn blocked(
		reason: BlockReason,
		reference_idx: usize,
		reference: ReferenceId,
	) -> Self {
		Self::Blocked {
			reason,
			reference,
			reference_idx,
		}
	}

	pub(in crate::linkage) fn external(
		origin: ExternalOrigin,
		reference_idx: usize,
		reference: ReferenceId,
	) -> Self {
		Self::External {
			origin,
			reference,
			reference_idx,
			target: None,
		}
	}

	pub(in crate::linkage) fn external_target(
		origin: ExternalOrigin,
		reference_idx: usize,
		reference: ReferenceId,
		target: Moniker,
	) -> Self {
		Self::External {
			origin,
			reference,
			reference_idx,
			target: Some(target),
		}
	}

	pub(in crate::linkage) fn reference_idx(&self) -> usize {
		match self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				resolution.reference_idx
			}
			Self::External { reference_idx, .. }
			| Self::Blocked { reference_idx, .. }
			| Self::Dynamic { reference_idx, .. }
			| Self::Unknown { reference_idx, .. } => *reference_idx,
		}
	}

	pub(in crate::linkage) fn semantic_pending_reference_idx(&self) -> Option<usize> {
		match self {
			Self::Candidate {
				reason: CandidateReason::WeakNameMatch,
				resolution,
			} => Some(resolution.reference_idx),
			Self::Unknown {
				reason: UnknownReason::NoCandidate | UnknownReason::IncompleteExtractorMetadata,
				reference_idx,
				..
			} => Some(*reference_idx),
			_ => None,
		}
	}

	pub(in crate::linkage) fn semantic_type_refinable_reference_idx(&self) -> Option<usize> {
		match self {
			Self::Candidate { resolution, .. } => Some(resolution.reference_idx),
			_ => self.semantic_pending_reference_idx(),
		}
	}

	pub(in crate::linkage) fn reference(&self) -> &ReferenceId {
		match self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				&resolution.reference
			}
			Self::External { reference, .. }
			| Self::Blocked { reference, .. }
			| Self::Dynamic { reference, .. }
			| Self::Unknown { reference, .. } => reference,
		}
	}

	pub(in crate::linkage) fn linkage_targets(&self) -> Option<&SymbolSet> {
		match self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				Some(&resolution.targets)
			}
			Self::Dynamic { candidates, .. } => Some(candidates),
			Self::External { .. } | Self::Blocked { .. } | Self::Unknown { .. } => None,
		}
	}

	pub(in crate::linkage) fn set_reference_idx(&mut self, next_reference_idx: usize) {
		match &mut *self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				resolution.reference_idx = next_reference_idx;
			}
			Self::External { reference_idx, .. }
			| Self::Blocked { reference_idx, .. }
			| Self::Dynamic { reference_idx, .. }
			| Self::Unknown { reference_idx, .. } => *reference_idx = next_reference_idx,
		}
	}

	pub(in crate::linkage) fn set_reference(
		&mut self,
		next_reference: ReferenceId,
		next_reference_idx: usize,
	) {
		match &mut *self {
			Self::Unique { resolution } | Self::Candidate { resolution, .. } => {
				resolution.reference = next_reference;
				resolution.reference_idx = next_reference_idx;
			}
			Self::External {
				reference,
				reference_idx,
				..
			}
			| Self::Blocked {
				reference,
				reference_idx,
				..
			}
			| Self::Dynamic {
				reference,
				reference_idx,
				..
			}
			| Self::Unknown {
				reference,
				reference_idx,
				..
			} => {
				*reference = next_reference;
				*reference_idx = next_reference_idx;
			}
		}
	}
}

#[derive(Default)]
pub(in crate::linkage) struct LinkageReportProjection {
	resolved: ResolvedLinkProjection,
	candidates: CandidateLinkProjection,
	external: ExternalLinkProjection,
	dynamic: DynamicLinkProjection,
	blocked: BlockedLinkProjection,
	unresolved: UnresolvedLinkProjection,
}

impl LinkageReportProjection {
	fn from_decisions(
		decisions: &[ReferenceLinkageDecision],
		references: &RecordTable<ReferenceRecord>,
		identity: &LocalIdentityResolver,
		symbols: &SymbolOrdinalCatalog,
	) -> Self {
		let capacity = LinkageProjectionCapacity::from_decisions(decisions);
		decisions
			.iter()
			.map(|decision| {
				LinkageDecisionProjection::from_decision(decision, references, identity, symbols)
			})
			.fold(Self::with_capacity(capacity), Self::collect)
	}

	fn with_capacity(capacity: LinkageProjectionCapacity) -> Self {
		Self {
			resolved: ResolvedLinkProjection::with_capacity(capacity.resolved_edges),
			candidates: CandidateLinkProjection::with_capacity(capacity.candidate_refs),
			external: ExternalLinkProjection::with_capacity(capacity.external_refs),
			dynamic: DynamicLinkProjection::with_capacity(capacity.dynamic_refs),
			blocked: BlockedLinkProjection::with_capacity(capacity.blocked_refs),
			unresolved: UnresolvedLinkProjection::with_capacity(capacity.unresolved_refs),
		}
	}

	fn collect(mut self, decision: LinkageDecisionProjection) -> Self {
		match decision {
			LinkageDecisionProjection::Resolved(resolved) => self.resolved.collect(resolved),
			LinkageDecisionProjection::Candidate(candidate) => self.candidates.collect(candidate),
			LinkageDecisionProjection::External(external) => self.external.collect(external),
			LinkageDecisionProjection::Dynamic(dynamic) => self.dynamic.collect(dynamic),
			LinkageDecisionProjection::Blocked(reference) => self.blocked.collect(reference),
			LinkageDecisionProjection::Unresolved(reference) => self.unresolved.collect(reference),
		}
		self
	}

	pub(in crate::linkage) fn into_snapshot(
		self,
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
	) -> LinkageSnapshot {
		let mut resolved = self.resolved.edges;
		let mut candidates = self.candidates.references;
		let mut external = self.external.references;
		let mut dynamic = self.dynamic.references;
		let mut blocked = self.blocked.references;
		let mut manifest_blocked = blocked
			.iter()
			.filter(|reference| reference.reason == UnresolvedReason::ManifestBlocked)
			.cloned()
			.collect::<Vec<_>>();
		let mut unresolved = self.unresolved.references;
		resolved.shrink_to_fit();
		candidates.shrink_to_fit();
		external.shrink_to_fit();
		dynamic.shrink_to_fit();
		blocked.shrink_to_fit();
		manifest_blocked.shrink_to_fit();
		unresolved.shrink_to_fit();
		let read_index = crate::snapshot::LinkageReadIndexHandle::from_edges(&resolved);
		LinkageSnapshot {
			generation,
			index_generation,
			resolved_refs: self.resolved.resolved_refs,
			candidate_refs: self.candidates.candidate_refs,
			external_refs: self.external.external_refs,
			dynamic_refs: self.dynamic.dynamic_refs,
			blocked_refs: self.blocked.blocked_refs,
			manifest_blocked_refs: manifest_blocked.len(),
			unresolved_refs: self.unresolved.unresolved_refs,
			ambiguous_refs: self.candidates.candidate_refs,
			resolved,
			candidates,
			external,
			dynamic,
			blocked,
			manifest_blocked,
			unresolved,
			read_index,
		}
	}
}

struct LinkageProjectionCapacity {
	resolved_edges: usize,
	candidate_refs: usize,
	external_refs: usize,
	dynamic_refs: usize,
	blocked_refs: usize,
	unresolved_refs: usize,
}

impl LinkageProjectionCapacity {
	fn from_decisions(decisions: &[ReferenceLinkageDecision]) -> Self {
		decisions.iter().fold(
			Self {
				resolved_edges: 0,
				candidate_refs: 0,
				external_refs: 0,
				dynamic_refs: 0,
				blocked_refs: 0,
				unresolved_refs: 0,
			},
			|mut capacity, decision| {
				match decision {
					ReferenceLinkageDecision::Unique { resolution } => {
						capacity.resolved_edges += resolution.targets.len();
					}
					ReferenceLinkageDecision::Candidate { .. } => {
						capacity.candidate_refs += 1;
					}
					ReferenceLinkageDecision::Dynamic { .. } => {
						capacity.dynamic_refs += 1;
					}
					ReferenceLinkageDecision::Blocked { .. } => {
						capacity.blocked_refs += 1;
					}
					ReferenceLinkageDecision::Unknown { .. } => {
						capacity.unresolved_refs += 1;
					}
					ReferenceLinkageDecision::External { .. } => {
						capacity.external_refs += 1;
					}
				}
				capacity
			},
		)
	}
}

enum LinkageDecisionProjection {
	Resolved(LinkageEdge),
	Candidate(CandidateReference),
	External(ExternalReference),
	Dynamic(DynamicReference),
	Blocked(UnresolvedReference),
	Unresolved(UnresolvedReference),
}

impl LinkageDecisionProjection {
	fn from_decision(
		decision: &ReferenceLinkageDecision,
		references: &RecordTable<ReferenceRecord>,
		identity: &LocalIdentityResolver,
		symbols: &SymbolOrdinalCatalog,
	) -> Self {
		match decision {
			ReferenceLinkageDecision::Unique { resolution } => {
				project_unique_decision(resolution, references, symbols)
			}
			ReferenceLinkageDecision::Candidate { reason, resolution } => {
				project_candidate_decision(*reason, resolution, references, symbols)
			}
			ReferenceLinkageDecision::Blocked {
				reason,
				reference_idx,
				..
			} => project_blocked_decision(*reason, *reference_idx, references),
			ReferenceLinkageDecision::Unknown {
				reason,
				reference_idx,
				..
			} => project_unknown_decision(reason, *reference_idx, references),
			ReferenceLinkageDecision::External {
				origin,
				reference_idx,
				target,
				..
			} => project_external_decision(
				*origin,
				*reference_idx,
				target.as_ref(),
				references,
				identity,
			),
			ReferenceLinkageDecision::Dynamic {
				reason,
				reference_idx,
				candidates,
				..
			} => project_dynamic_decision(*reason, *reference_idx, candidates, references, symbols),
		}
	}
}

fn project_unique_decision(
	resolution: &ResolutionDecision,
	references: &RecordTable<ReferenceRecord>,
	symbols: &SymbolOrdinalCatalog,
) -> LinkageDecisionProjection {
	let reference = &references[resolution.reference_idx];
	let mut targets = symbols.ids(&resolution.targets);
	match targets.pop() {
		Some(target) if targets.is_empty() => LinkageDecisionProjection::Resolved(
			LinkageEdge::with_evidence(reference.id, target, resolution.evidence),
		),
		Some(target) => {
			targets.push(target);
			LinkageDecisionProjection::Candidate(CandidateReference::new(
				reference.id,
				targets,
				CandidateReason::MultipleTargets,
				CandidateScope::Unknown,
				resolution.evidence,
			))
		}
		None => LinkageDecisionProjection::Unresolved(unresolved_reference(
			reference,
			UnresolvedReason::NoCandidate,
		)),
	}
}

fn project_candidate_decision(
	reason: CandidateReason,
	resolution: &ResolutionDecision,
	references: &RecordTable<ReferenceRecord>,
	symbols: &SymbolOrdinalCatalog,
) -> LinkageDecisionProjection {
	LinkageDecisionProjection::Candidate(CandidateReference::new(
		references[resolution.reference_idx].id,
		symbols.ids(&resolution.targets),
		reason,
		resolution.scope.into(),
		resolution.evidence,
	))
}

fn project_blocked_decision(
	reason: BlockReason,
	reference_idx: usize,
	references: &RecordTable<ReferenceRecord>,
) -> LinkageDecisionProjection {
	LinkageDecisionProjection::Blocked(unresolved_reference(
		&references[reference_idx],
		reason.unresolved_reason(),
	))
}

fn project_unknown_decision(
	reason: &UnknownReason,
	reference_idx: usize,
	references: &RecordTable<ReferenceRecord>,
) -> LinkageDecisionProjection {
	LinkageDecisionProjection::Unresolved(unresolved_reference(
		&references[reference_idx],
		reason.unresolved_reason(),
	))
}

fn project_external_decision(
	origin: ExternalOrigin,
	reference_idx: usize,
	target: Option<&Moniker>,
	references: &RecordTable<ReferenceRecord>,
	identity: &LocalIdentityResolver,
) -> LinkageDecisionProjection {
	LinkageDecisionProjection::External(external_reference(
		&references[reference_idx],
		origin,
		target,
		identity,
	))
}

fn project_dynamic_decision(
	reason: DynamicReason,
	reference_idx: usize,
	candidates: &SymbolSet,
	references: &RecordTable<ReferenceRecord>,
	symbols: &SymbolOrdinalCatalog,
) -> LinkageDecisionProjection {
	let reference = &references[reference_idx];
	LinkageDecisionProjection::Dynamic(DynamicReference::new(
		reference.id,
		Arc::clone(&reference.target_identity),
		reason,
		symbols.ids(candidates),
	))
}

#[derive(Default)]
struct ResolvedLinkProjection {
	resolved_refs: usize,
	edges: Vec<LinkageEdge>,
}

impl ResolvedLinkProjection {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			resolved_refs: 0,
			edges: Vec::with_capacity(capacity),
		}
	}

	fn collect(&mut self, resolved: LinkageEdge) {
		self.resolved_refs += 1;
		self.edges.push(resolved);
	}
}

#[derive(Default)]
struct CandidateLinkProjection {
	candidate_refs: usize,
	references: Vec<CandidateReference>,
}

impl CandidateLinkProjection {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			candidate_refs: 0,
			references: Vec::with_capacity(capacity),
		}
	}

	fn collect(&mut self, reference: CandidateReference) {
		self.candidate_refs += 1;
		self.references.push(reference);
	}
}

#[derive(Default)]
struct ExternalLinkProjection {
	external_refs: usize,
	references: Vec<ExternalReference>,
}

impl ExternalLinkProjection {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			external_refs: 0,
			references: Vec::with_capacity(capacity),
		}
	}

	fn collect(&mut self, reference: ExternalReference) {
		self.external_refs += 1;
		self.references.push(reference);
	}
}

#[derive(Default)]
struct DynamicLinkProjection {
	dynamic_refs: usize,
	references: Vec<DynamicReference>,
}

impl DynamicLinkProjection {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			dynamic_refs: 0,
			references: Vec::with_capacity(capacity),
		}
	}

	fn collect(&mut self, reference: DynamicReference) {
		self.dynamic_refs += 1;
		self.references.push(reference);
	}
}

#[derive(Default)]
struct BlockedLinkProjection {
	blocked_refs: usize,
	references: Vec<UnresolvedReference>,
}

impl BlockedLinkProjection {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			blocked_refs: 0,
			references: Vec::with_capacity(capacity),
		}
	}

	fn collect(&mut self, reference: UnresolvedReference) {
		self.blocked_refs += 1;
		self.references.push(reference);
	}
}

#[derive(Default)]
struct UnresolvedLinkProjection {
	unresolved_refs: usize,
	references: Vec<UnresolvedReference>,
}

impl UnresolvedLinkProjection {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			unresolved_refs: 0,
			references: Vec::with_capacity(capacity),
		}
	}

	fn collect(&mut self, reference: UnresolvedReference) {
		self.unresolved_refs += 1;
		self.references.push(reference);
	}
}

fn unresolved_reference(
	reference: &ReferenceRecord,
	reason: UnresolvedReason,
) -> UnresolvedReference {
	UnresolvedReference::new(reference.id, Arc::clone(&reference.target_identity), reason)
}

fn external_reference(
	reference: &ReferenceRecord,
	origin: ExternalOrigin,
	target: Option<&Moniker>,
	identity: &LocalIdentityResolver,
) -> ExternalReference {
	ExternalReference::new(
		reference.id,
		target
			.map(|target| identity.moniker_uri(target))
			.unwrap_or_else(|| reference.target_identity.to_string()),
		origin,
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::snapshot::{CandidateReason, SourceId, SymbolId};

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
			.map(|idx| {
				symbols.push(
					SymbolId::at(0, idx + 1),
					Arc::from(format!(
						"code+moniker://./lang:python/module:sample/class:Target{idx}"
					)),
				)
			})
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
}
