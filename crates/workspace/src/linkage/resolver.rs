use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::decision::{
	LinkageDecisionLog, ReferenceLinkageDecision, ResolutionScope, UnknownReason,
};
use crate::linkage::manifest::ManifestPolicy;
use crate::linkage::query::LinkageQuery;
use crate::linkage::scope::{GlobalScopeResolver, LocalScopeResolver};
use crate::linkage::semantic::SemanticLinkage;
use crate::snapshot::{
	CodeIndex, LinkageGraph, ReferenceRecord, WorkspaceFailure, WorkspaceResource, WorkspaceResult,
};
use crate::source::{CodeIndexMaterial, LocalResourceCache};

pub trait LinkagePort {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageGraph>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LinkageTimings {
	pub candidate_index: Duration,
	pub manifest_policy: Duration,
	pub resolve_references: Duration,
	pub semantic_prepare: Duration,
	pub semantic_enhance: Duration,
	pub project_report: Duration,
	pub total: Duration,
}

pub struct TimedLinkageGraph {
	pub graph: LinkageGraph,
	pub timings: LinkageTimings,
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
		Ok(self.resolve_linkage_with_timings(index)?.graph)
	}
}

impl LocalLinkage {
	pub fn resolve_linkage_with_timings(
		&mut self,
		index: &CodeIndex,
	) -> WorkspaceResult<TimedLinkageGraph> {
		let total_timer = Instant::now();
		let material = self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::LinkageGraph,
				"code index material is unavailable",
			)
		})?;
		let generation = self.cache.next_generation();
		let resolver = LinkageResolver::new(&material);
		let LinkageResolution {
			decision_log,
			mut timings,
		} = resolver.resolve(index);
		let report_timer = Instant::now();
		let graph = LinkageGraph::from_report(decision_log.project_report(
			generation,
			index.generation,
			&index.references,
		));
		timings.project_report = report_timer.elapsed();
		timings.total = total_timer.elapsed();
		Ok(TimedLinkageGraph { graph, timings })
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

	fn resolve(&self, index: &CodeIndex) -> LinkageResolution {
		let mut timings = LinkageTimings::default();
		let candidate_timer = Instant::now();
		let candidates = CandidateCatalog::new(self.material);
		timings.candidate_index = candidate_timer.elapsed();
		let manifest_timer = Instant::now();
		let manifests = ManifestPolicy::build(self.material);
		timings.manifest_policy = manifest_timer.elapsed();
		let resolve_timer = Instant::now();
		let mut decisions = index
			.references
			.par_iter()
			.enumerate()
			.map(|(reference_idx, reference)| {
				self.resolve_reference(reference_idx, reference, &candidates, &manifests)
			})
			.collect::<Vec<_>>();
		timings.resolve_references = resolve_timer.elapsed();
		let semantic_prepare_timer = Instant::now();
		let semantic_linkage = SemanticLinkage::new(self.material);
		timings.semantic_prepare = semantic_prepare_timer.elapsed();
		let semantic_timer = Instant::now();
		semantic_linkage.enhance(&mut decisions, &index.references);
		timings.semantic_enhance = semantic_timer.elapsed();
		let decision_log = LinkageDecisionLog::new(decisions);
		LinkageResolution {
			decision_log,
			timings,
		}
	}

	fn resolve_reference(
		&self,
		reference_idx: usize,
		reference: &ReferenceRecord,
		candidates: &CandidateCatalog<'_>,
		manifests: &ManifestPolicy,
	) -> ReferenceLinkageDecision {
		let Some(query) = LinkageQuery::new(reference, self.material) else {
			return ReferenceLinkageDecision::unknown(UnknownReason::MissingQuery, reference_idx);
		};

		let local_targets = self.local.resolve(&query, candidates);
		if !local_targets.is_empty() {
			return ReferenceLinkageDecision::resolved(
				ResolutionScope::Local,
				reference_idx,
				local_targets,
			);
		}

		let global_targets = self.global.resolve(&query, candidates);
		let global_decision = manifests.evaluate_global_targets(&query, global_targets);
		if let Some(decision) = global_decision.for_reference(reference_idx) {
			return decision;
		}

		ReferenceLinkageDecision::unknown(UnknownReason::NoCandidate, reference_idx)
	}
}

struct LinkageResolution {
	decision_log: LinkageDecisionLog,
	timings: LinkageTimings,
}
