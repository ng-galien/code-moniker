use std::sync::Arc;

use code_moniker_core::core::moniker::Moniker;

use super::{
	BlockReason, ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision, UnknownReason,
};
use crate::linkage::catalog::{SymbolOrdinalCatalog, SymbolSet};
use crate::snapshot::{
	CandidateReason, CandidateReference, CandidateScope, DynamicReason, DynamicReference,
	ExternalReference, LinkageEdge, LinkageSnapshot, RecordTable, ReferenceRecord,
	ResourceGeneration, UnresolvedReason, UnresolvedReference,
};
use crate::source::LocalIdentityResolver;

pub(in crate::linkage) fn project_decisions(
	decisions: &[ReferenceLinkageDecision],
	references: &RecordTable<ReferenceRecord>,
	identity: &LocalIdentityResolver,
	symbols: &SymbolOrdinalCatalog,
) -> LinkageReportProjection {
	LinkageReportProjection::from_decisions(decisions, references, identity, symbols)
}

pub(in crate::linkage) struct LinkageReportProjection {
	resolved: Vec<LinkageEdge>,
	candidates: Vec<CandidateReference>,
	external: Vec<ExternalReference>,
	dynamic: Vec<DynamicReference>,
	blocked: Vec<UnresolvedReference>,
	unresolved: Vec<UnresolvedReference>,
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
			resolved: Vec::with_capacity(capacity.resolved_edges),
			candidates: Vec::with_capacity(capacity.candidate_refs),
			external: Vec::with_capacity(capacity.external_refs),
			dynamic: Vec::with_capacity(capacity.dynamic_refs),
			blocked: Vec::with_capacity(capacity.blocked_refs),
			unresolved: Vec::with_capacity(capacity.unresolved_refs),
		}
	}

	fn collect(mut self, decision: LinkageDecisionProjection) -> Self {
		match decision {
			LinkageDecisionProjection::Resolved(resolved) => self.resolved.push(resolved),
			LinkageDecisionProjection::Candidate(candidate) => self.candidates.push(candidate),
			LinkageDecisionProjection::External(external) => self.external.push(external),
			LinkageDecisionProjection::Dynamic(dynamic) => self.dynamic.push(dynamic),
			LinkageDecisionProjection::Blocked(reference) => self.blocked.push(reference),
			LinkageDecisionProjection::Unresolved(reference) => self.unresolved.push(reference),
		}
		self
	}

	pub(in crate::linkage) fn into_snapshot(
		self,
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
	) -> LinkageSnapshot {
		let Self {
			mut resolved,
			mut candidates,
			mut external,
			mut dynamic,
			mut blocked,
			mut unresolved,
		} = self;
		let mut manifest_blocked = blocked
			.iter()
			.filter(|reference| reference.reason == UnresolvedReason::ManifestBlocked)
			.cloned()
			.collect::<Vec<_>>();
		let resolved_refs = resolved.len();
		let candidate_refs = candidates.len();
		let external_refs = external.len();
		let dynamic_refs = dynamic.len();
		let blocked_refs = blocked.len();
		let unresolved_refs = unresolved.len();
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
			resolved_refs,
			candidate_refs,
			external_refs,
			dynamic_refs,
			blocked_refs,
			manifest_blocked_refs: manifest_blocked.len(),
			unresolved_refs,
			ambiguous_refs: candidate_refs,
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
					ReferenceLinkageDecision::Candidate { .. } => capacity.candidate_refs += 1,
					ReferenceLinkageDecision::Dynamic { .. } => capacity.dynamic_refs += 1,
					ReferenceLinkageDecision::Blocked { .. } => capacity.blocked_refs += 1,
					ReferenceLinkageDecision::Unknown { .. } => capacity.unresolved_refs += 1,
					ReferenceLinkageDecision::External { .. } => capacity.external_refs += 1,
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
