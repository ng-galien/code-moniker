use std::sync::Arc;

use code_moniker_core::core::moniker::Moniker;

use super::{ExternalOrigin, ReferenceLinkageDecision, ResolutionDecision};
use crate::linkage::catalog::SymbolOrdinalCatalog;
use crate::snapshot::{
	CandidateReason, CandidateReference, CandidateScope, DynamicReference, ExternalReference,
	LinkageEdge, LinkageSnapshot, RecordTable, ReferenceRecord, ResourceGeneration,
	UnresolvedReason, UnresolvedReference,
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

#[derive(Default)]
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
		let mut projection = Self::default();
		for decision in decisions {
			match decision {
				ReferenceLinkageDecision::Unique { resolution } => {
					project_unique_decision(&mut projection, resolution, references, symbols);
				}
				ReferenceLinkageDecision::Candidate { reason, resolution } => {
					projection.candidates.push(CandidateReference::new(
						references[resolution.reference_idx].id,
						symbols.ids(&resolution.targets),
						*reason,
						resolution.scope.into(),
						resolution.evidence,
					));
				}
				ReferenceLinkageDecision::Blocked {
					reason,
					reference_idx,
					..
				} => projection.blocked.push(unresolved_reference(
					&references[*reference_idx],
					reason.unresolved_reason(),
				)),
				ReferenceLinkageDecision::Unknown {
					reason,
					reference_idx,
					..
				} => projection.unresolved.push(unresolved_reference(
					&references[*reference_idx],
					reason.unresolved_reason(),
				)),
				ReferenceLinkageDecision::External {
					origin,
					reference_idx,
					target,
					..
				} => projection.external.push(external_reference(
					&references[*reference_idx],
					*origin,
					target.as_ref(),
					identity,
				)),
				ReferenceLinkageDecision::Dynamic {
					reason,
					reference_idx,
					candidates,
					..
				} => {
					let reference = &references[*reference_idx];
					projection.dynamic.push(DynamicReference::new(
						reference.id,
						Arc::clone(&reference.target_identity),
						*reason,
						symbols.ids(candidates),
					));
				}
			}
		}
		projection
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
		#[cfg(test)]
		let read_index = crate::snapshot::LinkageReadIndexHandle::from_edges(&resolved);
		#[cfg(not(test))]
		let read_index = crate::snapshot::LinkageReadIndexHandle::default();
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

fn project_unique_decision(
	projection: &mut LinkageReportProjection,
	resolution: &ResolutionDecision,
	references: &RecordTable<ReferenceRecord>,
	symbols: &SymbolOrdinalCatalog,
) {
	let reference = &references[resolution.reference_idx];
	let mut targets = symbols.ids(&resolution.targets);
	match targets.pop() {
		Some(target) if targets.is_empty() => projection.resolved.push(LinkageEdge::with_evidence(
			reference.id,
			target,
			resolution.evidence,
		)),
		Some(target) => {
			targets.push(target);
			projection.candidates.push(CandidateReference::new(
				reference.id,
				targets,
				CandidateReason::MultipleTargets,
				CandidateScope::Unknown,
				resolution.evidence,
			));
		}
		None => projection.unresolved.push(unresolved_reference(
			reference,
			UnresolvedReason::NoCandidate,
		)),
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
