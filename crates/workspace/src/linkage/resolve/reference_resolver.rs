use crate::linkage::binding::{ReferenceLinkageDecision, ResolutionScope, UnknownReason};
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::SymbolSet;
use crate::linkage::catalog::{LinkageQuery, ReferenceLocation};
use crate::linkage::resolve::manifest::GlobalTargetPolicy;
use crate::linkage::resolve::source_groups::SourceGroupPolicy;
use crate::linkage::resolve::{GlobalScopeResolver, LocalScopeResolver, ManifestPolicy};
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
		let mut global_decision = policies.manifests.evaluate_global_targets(
			query,
			global_targets.clone(),
			policies.candidates,
		);
		self.allow_declared_group_targets(
			query.source_file,
			&global_targets,
			policies,
			&mut global_decision,
		);
		global_decision.for_reference(site.reference_idx, site.reference)
	}

	fn allow_declared_group_targets(
		&self,
		source_file: usize,
		global_targets: &SymbolSet,
		policies: &LinkagePolicies<'_>,
		global_decision: &mut GlobalTargetPolicy,
	) {
		let Some(source_group) = policies.source_groups.group_of(self.material, source_file) else {
			return;
		};
		for symbol in global_targets.iter() {
			let Some(candidate) = policies.candidates.candidate(symbol) else {
				continue;
			};
			let target_group = policies
				.source_groups
				.group_of(self.material, candidate.source_file);
			if target_group == Some(source_group) {
				global_decision.allow(symbol);
			}
		}
	}
}
