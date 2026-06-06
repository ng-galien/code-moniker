use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::decision::{ReferenceLinkageDecision, ResolutionScope, UnknownReason};
use crate::linkage::manifest::ManifestPolicy;
use crate::linkage::query::{LinkageQuery, ReferenceLocation};
use crate::linkage::scope::{GlobalScopeResolver, LocalScopeResolver};
use crate::snapshot::ReferenceRecord;
use crate::source::CodeIndexMaterial;

pub(super) struct ReferenceResolver<'a> {
	material: &'a CodeIndexMaterial,
	local: LocalScopeResolver,
	global: GlobalScopeResolver,
}

impl<'a> ReferenceResolver<'a> {
	pub(super) fn new(material: &'a CodeIndexMaterial) -> Self {
		Self {
			material,
			local: LocalScopeResolver,
			global: GlobalScopeResolver,
		}
	}

	pub(super) fn resolve_reference(
		&self,
		reference_idx: usize,
		reference: &ReferenceRecord,
		location: Option<ReferenceLocation>,
		candidates: &CandidateCatalog<'_>,
		manifests: &ManifestPolicy,
	) -> ReferenceLinkageDecision {
		let Some(location) = location else {
			return ReferenceLinkageDecision::unknown(
				UnknownReason::MissingQuery,
				reference_idx,
				reference.id.clone(),
			);
		};
		let Some(query) = LinkageQuery::at(reference, self.material, location) else {
			return ReferenceLinkageDecision::unknown(
				UnknownReason::MissingQuery,
				reference_idx,
				reference.id.clone(),
			);
		};

		let local_targets = self.local.resolve(&query, candidates);
		if !local_targets.is_empty() {
			return ReferenceLinkageDecision::resolved(
				ResolutionScope::Local,
				reference_idx,
				reference.id.clone(),
				local_targets,
			);
		}

		let global_targets = self.global.resolve(&query, candidates);
		let global_decision = manifests.evaluate_global_targets(&query, global_targets, candidates);
		if let Some(decision) = global_decision.for_reference(reference_idx, reference) {
			return decision;
		}

		ReferenceLinkageDecision::unknown(
			UnknownReason::NoCandidate,
			reference_idx,
			reference.id.clone(),
		)
	}
}
