use crate::linkage::binding::{ReferenceLinkageDecision, ResolutionScope, UnknownReason};
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::{LinkageQuery, ReferenceLocation};
use crate::linkage::resolve::{GlobalScopeResolver, LocalScopeResolver, ManifestPolicy};
use crate::linkage::source_groups::SourceGroupPolicy;
use crate::snapshot::ReferenceRecord;
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) struct LinkagePolicies<'a> {
	pub(in crate::linkage) candidates: &'a CandidateCatalog,
	pub(in crate::linkage) manifests: &'a ManifestPolicy,
	pub(in crate::linkage) source_groups: &'a SourceGroupPolicy,
}

#[derive(Clone, Copy)]
struct ReferenceSite<'a> {
	reference_idx: usize,
	reference: &'a ReferenceRecord,
}

impl ReferenceSite<'_> {
	fn unknown(&self, reason: UnknownReason) -> ReferenceLinkageDecision {
		ReferenceLinkageDecision::unknown(reason, self.reference_idx, self.reference.id)
	}
}

pub(in crate::linkage) struct ReferenceResolver<'a> {
	material: &'a CodeIndexMaterial,
	local: LocalScopeResolver,
	global: GlobalScopeResolver,
}

impl<'a> ReferenceResolver<'a> {
	pub(in crate::linkage) fn new(material: &'a CodeIndexMaterial) -> Self {
		Self {
			material,
			local: LocalScopeResolver,
			global: GlobalScopeResolver,
		}
	}

	pub(in crate::linkage) fn resolve_reference(
		&self,
		reference_idx: usize,
		reference: &ReferenceRecord,
		location: Option<ReferenceLocation>,
		policies: &LinkagePolicies<'_>,
	) -> ReferenceLinkageDecision {
		let site = ReferenceSite {
			reference_idx,
			reference,
		};
		let Some(location) = location else {
			return site.unknown(UnknownReason::MissingQuery);
		};
		let Some(query) = LinkageQuery::at(reference, self.material, location) else {
			return site.unknown(UnknownReason::MissingQuery);
		};

		let local_targets = self.local.resolve(&query, policies.candidates);
		if !local_targets.is_empty() {
			return ReferenceLinkageDecision::resolved(
				ResolutionScope::Local,
				reference_idx,
				reference.id,
				local_targets,
			);
		}

		if let Some(decision) = self.resolve_global(&query, site, policies) {
			return decision;
		}

		site.unknown(UnknownReason::NoCandidate)
	}

	fn resolve_global(
		&self,
		query: &LinkageQuery<'_>,
		site: ReferenceSite<'_>,
		policies: &LinkagePolicies<'_>,
	) -> Option<ReferenceLinkageDecision> {
		let global_targets = self.global.resolve(query, policies.candidates);
		let global_decision = policies.manifests.evaluate_global_targets(
			query,
			global_targets,
			policies.candidates,
			|target_file| {
				policies.source_groups.link_permission(
					self.material,
					query.source_file,
					target_file,
				)
			},
		);
		global_decision.for_reference(site.reference_idx, site.reference)
	}
}
