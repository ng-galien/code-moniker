use crate::workspace::linkage::candidate::CandidateCatalog;
use crate::workspace::linkage::decision::{
	ExternalOrigin, LinkageDecisionLog, ReferenceLinkageDecision, ResolutionScope, UnknownReason,
};
use crate::workspace::linkage::manifest::ManifestPolicy;
use crate::workspace::linkage::query::LinkageQuery;
use crate::workspace::linkage::scope::{GlobalScopeResolver, LocalScopeResolver};
use crate::workspace::snapshot::{
	CodeIndex, LinkageGraph, ReferenceRecord, WorkspaceFailure, WorkspaceResource, WorkspaceResult,
};
use crate::workspace::source::{CodeIndexMaterial, LocalResourceCache};

pub trait LinkagePort {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageGraph>;
}

pub struct LocalLinkage {
	cache: LocalResourceCache,
}

impl LocalLinkage {
	pub fn new(cache: LocalResourceCache) -> Self {
		Self { cache }
	}
}

impl LinkagePort for LocalLinkage {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageGraph> {
		let material = self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::LinkageGraph,
				"code index material is unavailable",
			)
		})?;
		let generation = self.cache.next_generation();
		let resolver = LinkageResolver::new(&material);
		let decision_log = resolver.resolve(index);
		Ok(LinkageGraph::from_report(
			decision_log.project_report(generation, index.generation),
		))
	}
}

struct LinkageResolver<'a> {
	material: &'a CodeIndexMaterial,
	local: LocalScopeResolver,
	global: GlobalScopeResolver,
}

impl<'a> LinkageResolver<'a> {
	fn new(material: &'a CodeIndexMaterial) -> Self {
		Self {
			material,
			local: LocalScopeResolver,
			global: GlobalScopeResolver,
		}
	}

	fn resolve(&self, index: &CodeIndex) -> LinkageDecisionLog {
		let candidates = CandidateCatalog::new(self.material);
		let manifests = ManifestPolicy::build(self.material);
		LinkageDecisionLog::new(
			index
				.references
				.iter()
				.map(|reference| self.resolve_reference(reference, &candidates, &manifests))
				.collect(),
		)
	}

	fn resolve_reference(
		&self,
		reference: &ReferenceRecord,
		candidates: &CandidateCatalog<'_>,
		manifests: &ManifestPolicy,
	) -> ReferenceLinkageDecision {
		let Some(query) = LinkageQuery::new(reference, self.material) else {
			if reference.confidence.as_deref() == Some("external") {
				return ReferenceLinkageDecision::external(ExternalOrigin::Dependency, reference);
			}
			return ReferenceLinkageDecision::unknown(UnknownReason::MissingQuery, reference);
		};

		let local_targets = self.local.resolve(&query, candidates);
		if !local_targets.is_empty() {
			return ReferenceLinkageDecision::resolved(
				ResolutionScope::Local,
				reference,
				local_targets,
			);
		}

		let global_targets = self.global.resolve(&query, candidates);
		let global_decision = manifests.evaluate_global_targets(&query, global_targets);
		if let Some(decision) = global_decision.for_reference(reference) {
			return decision;
		}

		if reference.confidence.as_deref() == Some("external") {
			return ReferenceLinkageDecision::external(ExternalOrigin::Dependency, reference);
		}
		ReferenceLinkageDecision::unknown(UnknownReason::NoCandidate, reference)
	}
}
