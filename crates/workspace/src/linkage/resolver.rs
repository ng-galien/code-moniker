use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::decision::{ReferenceLinkageDecision, ResolutionScope, UnknownReason};
use crate::linkage::gc::{LinkageGarbageCollector, LinkageRefreshImpact};
use crate::linkage::manifest::ManifestPolicy;
use crate::linkage::query::LinkageQuery;
use crate::linkage::scope::{GlobalScopeResolver, LocalScopeResolver};
use crate::linkage::semantic::SemanticLinkage;
use crate::linkage::store::LinkageStore;
use crate::snapshot::{
	CodeIndex, LinkageSnapshot, ReferenceRecord, WorkspaceFailure, WorkspaceResource,
	WorkspaceResult,
};
use crate::source::{CodeIndexMaterial, LocalResourceCache};

pub trait LinkagePort {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageSnapshot>;
	fn refresh_linkage(
		&mut self,
		current: &LinkageSnapshot,
		index: &CodeIndex,
		impact: LinkageRefreshImpact,
	) -> WorkspaceResult<LinkageSnapshot>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LinkageTimings {
	pub candidate_index: Duration,
	pub manifest_policy: Duration,
	pub resolve_references: Duration,
	pub semantic_prepare: Duration,
	pub semantic_enhance: Duration,
	pub project_snapshot: Duration,
	pub total: Duration,
}

pub struct TimedLinkageSnapshot {
	pub snapshot: LinkageSnapshot,
	pub timings: LinkageTimings,
}

pub struct LocalLinkage {
	cache: LocalResourceCache,
	store: Option<LinkageStore>,
}

impl LocalLinkage {
	pub fn new(cache: LocalResourceCache) -> Self {
		Self { cache, store: None }
	}
}

impl LinkagePort for LocalLinkage {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageSnapshot> {
		Ok(self.resolve_linkage_with_timings(index)?.snapshot)
	}

	fn refresh_linkage(
		&mut self,
		current: &LinkageSnapshot,
		index: &CodeIndex,
		impact: LinkageRefreshImpact,
	) -> WorkspaceResult<LinkageSnapshot> {
		if impact.is_empty() {
			if let Some(store) = &mut self.store {
				store.advance_index_generation(index.generation);
			}
			let mut snapshot = current.clone();
			snapshot.index_generation = index.generation;
			return Ok(snapshot);
		}
		let material = self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::LinkageSnapshot,
				"code index material is unavailable",
			)
		})?;
		let generation = self.cache.next_generation();
		let store =
			IncrementalLinkageRefresh::new(current, index, impact, &material, generation).run();
		let snapshot = store.project_snapshot(&index.references, &material.identity);
		self.store = Some(store);
		Ok(snapshot)
	}
}

impl LocalLinkage {
	pub fn resolve_linkage_with_timings(
		&mut self,
		index: &CodeIndex,
	) -> WorkspaceResult<TimedLinkageSnapshot> {
		let total_timer = Instant::now();
		let material = self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::LinkageSnapshot,
				"code index material is unavailable",
			)
		})?;
		let generation = self.cache.next_generation();
		let resolver = LinkageResolver::new(&material);
		let LinkageResolution { store, mut timings } = resolver.resolve(index, generation);
		let report_timer = Instant::now();
		let snapshot = store.project_snapshot(&index.references, &material.identity);
		timings.project_snapshot = report_timer.elapsed();
		timings.total = total_timer.elapsed();
		self.store = Some(store);
		Ok(TimedLinkageSnapshot { snapshot, timings })
	}
}

struct IncrementalLinkageRefresh<'a> {
	current: &'a LinkageSnapshot,
	index: &'a CodeIndex,
	impact: LinkageRefreshImpact,
	material: &'a CodeIndexMaterial,
	generation: crate::snapshot::ResourceGeneration,
}

impl<'a> IncrementalLinkageRefresh<'a> {
	fn new(
		current: &'a LinkageSnapshot,
		index: &'a CodeIndex,
		impact: LinkageRefreshImpact,
		material: &'a CodeIndexMaterial,
		generation: crate::snapshot::ResourceGeneration,
	) -> Self {
		Self {
			current,
			index,
			impact,
			material,
			generation,
		}
	}

	fn run(&self) -> LinkageStore {
		let candidates = CandidateCatalog::new(self.material);
		let sweep = LinkageGarbageCollector::new(
			self.current,
			&self.index.references,
			self.material,
			&candidates,
			&self.impact,
		)
		.collect();
		let changed = self.resolve_reference_decisions(sweep.reference_indexes(), &candidates);
		let mut decisions = sweep.into_decisions(changed);
		SemanticLinkage::new(self.material).enhance(&mut decisions, &self.index.references);
		LinkageStore::new(
			self.generation,
			self.index.generation,
			decisions,
			&self.index.references,
		)
	}

	fn resolve_reference_decisions(
		&self,
		reference_indexes: &[usize],
		candidates: &CandidateCatalog<'_>,
	) -> Vec<ReferenceLinkageDecision> {
		let resolver = LinkageResolver::new(self.material);
		let manifests = ManifestPolicy::build(self.material);
		self.indexes_to_references(reference_indexes)
			.par_iter()
			.map(|(reference_idx, reference)| {
				resolver.resolve_reference(*reference_idx, reference, candidates, &manifests)
			})
			.collect::<Vec<_>>()
	}

	fn indexes_to_references(&self, reference_indexes: &[usize]) -> Vec<(usize, &ReferenceRecord)> {
		reference_indexes
			.iter()
			.filter_map(|reference_idx| {
				self.index
					.references
					.get(*reference_idx)
					.map(|reference| (*reference_idx, reference))
			})
			.collect()
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

	fn resolve(
		&self,
		index: &CodeIndex,
		generation: crate::snapshot::ResourceGeneration,
	) -> LinkageResolution {
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
		LinkageResolution {
			store: LinkageStore::new(generation, index.generation, decisions, &index.references),
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
	store: LinkageStore,
	timings: LinkageTimings,
}
