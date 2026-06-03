use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::decision::{ReferenceLinkageDecision, ResolutionScope, UnknownReason};
use crate::linkage::gc::{LinkageGarbageCollector, LinkageRefreshImpact};
use crate::linkage::manifest::ManifestPolicy;
use crate::linkage::query::LinkageQuery;
use crate::linkage::scope::{GlobalScopeResolver, LocalScopeResolver};
use crate::linkage::semantic::SemanticLinkage;
use crate::linkage::store::{LinkageStore, LinkageStoreRefresh};
use crate::snapshot::{
	CodeIndex, LinkageSnapshot, ReferenceId, ReferenceRecord, ResourceGeneration, WorkspaceFailure,
	WorkspaceResource, WorkspaceResult,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LinkageRefreshTimings {
	pub candidate_index: Duration,
	pub garbage_collect: Duration,
	pub resolve_references: Duration,
	pub apply_store: Duration,
	pub semantic_enhance: Duration,
	pub rebuild_indexes: Duration,
	pub project_snapshot: Duration,
	pub total: Duration,
	pub stale_refs: usize,
	pub changed_refs: usize,
}

pub struct TimedLinkageRefresh {
	pub snapshot: LinkageSnapshot,
	pub timings: LinkageRefreshTimings,
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
		snapshot: &LinkageSnapshot,
		indexed: &CodeIndex,
		change: LinkageRefreshImpact,
	) -> WorkspaceResult<LinkageSnapshot> {
		Ok(self
			.refresh_linkage_with_timings(snapshot, indexed, change)?
			.snapshot)
	}
}

impl LocalLinkage {
	pub fn refresh_linkage_with_timings(
		&mut self,
		previous: &LinkageSnapshot,
		code_index: &CodeIndex,
		refresh_impact: LinkageRefreshImpact,
	) -> WorkspaceResult<TimedLinkageRefresh> {
		let total_timer = Instant::now();
		if refresh_impact.is_empty() {
			return Ok(refresh_empty_linkage(
				&mut self.store,
				previous,
				code_index,
				total_timer,
			));
		}
		let material = self.linkage_material(code_index)?;
		let generation = self.cache.next_generation();
		let store = self.store_for_incremental(previous, code_index, &material);
		Ok(LinkageRefreshRunner::new(
			store,
			IncrementalLinkageInput {
				index: code_index,
				impact: refresh_impact,
				material: &material,
				generation,
			},
			total_timer,
		)
		.run())
	}

	fn store_for_incremental(
		&mut self,
		previous: &LinkageSnapshot,
		code_index: &CodeIndex,
		index_material: &CodeIndexMaterial,
	) -> &mut LinkageStore {
		self.store.get_or_insert_with(|| {
			let candidates = CandidateCatalog::new(index_material);
			LinkageStore::from_snapshot(
				previous,
				&code_index.references,
				index_material,
				&candidates,
			)
		})
	}

	fn linkage_material(&self, index: &CodeIndex) -> WorkspaceResult<Arc<CodeIndexMaterial>> {
		self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::LinkageSnapshot,
				"code index material is unavailable",
			)
		})
	}

	pub fn resolve_linkage_with_timings(
		&mut self,
		index: &CodeIndex,
	) -> WorkspaceResult<TimedLinkageSnapshot> {
		let total_timer = Instant::now();
		let material = self.linkage_material(index)?;
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

fn refresh_empty_linkage(
	store: &mut Option<LinkageStore>,
	previous: &LinkageSnapshot,
	code_index: &CodeIndex,
	total_timer: Instant,
) -> TimedLinkageRefresh {
	if let Some(store) = store {
		store.advance_index_generation(code_index.generation);
	}
	let project_timer = Instant::now();
	let mut snapshot = previous.clone();
	snapshot.index_generation = code_index.generation;
	TimedLinkageRefresh {
		snapshot,
		timings: LinkageRefreshTimings {
			project_snapshot: project_timer.elapsed(),
			total: total_timer.elapsed(),
			..LinkageRefreshTimings::default()
		},
	}
}

struct IncrementalLinkageInput<'a> {
	index: &'a CodeIndex,
	impact: LinkageRefreshImpact,
	material: &'a CodeIndexMaterial,
	generation: ResourceGeneration,
}

struct LinkageRefreshRunner<'a> {
	store: &'a mut LinkageStore,
	input: IncrementalLinkageInput<'a>,
	total_timer: Instant,
}

impl<'a> LinkageRefreshRunner<'a> {
	fn new(
		store: &'a mut LinkageStore,
		input: IncrementalLinkageInput<'a>,
		total_timer: Instant,
	) -> Self {
		Self {
			store,
			input,
			total_timer,
		}
	}

	fn run(self) -> TimedLinkageRefresh {
		let mut timings = refresh_incremental_linkage(self.store, &self.input);
		let project_timer = Instant::now();
		let snapshot = self
			.store
			.project_snapshot(&self.input.index.references, &self.input.material.identity);
		timings.project_snapshot = project_timer.elapsed();
		timings.total = self.total_timer.elapsed();
		TimedLinkageRefresh { snapshot, timings }
	}
}

fn refresh_incremental_linkage(
	store: &mut LinkageStore,
	input: &IncrementalLinkageInput<'_>,
) -> LinkageRefreshTimings {
	let mut timings = LinkageRefreshTimings::default();
	let candidate_timer = Instant::now();
	let candidates = CandidateCatalog::new(input.material);
	timings.candidate_index = candidate_timer.elapsed();
	let gc_timer = Instant::now();
	let stale_references = LinkageGarbageCollector::new(
		store,
		&input.index.references,
		input.material,
		&candidates,
		&input.impact,
	)
	.collect();
	timings.garbage_collect = gc_timer.elapsed();
	timings.stale_refs = stale_references.len();
	let reference_indexes = reference_indexes_for(&input.index.references, &stale_references);
	timings.changed_refs = reference_indexes.len();
	let resolve_timer = Instant::now();
	let changed = resolve_reference_decisions(input, &reference_indexes, &candidates);
	timings.resolve_references = resolve_timer.elapsed();
	let apply_timer = Instant::now();
	store.apply_refresh(LinkageStoreRefresh {
		generation: input.generation,
		index_generation: input.index.generation,
		stale_references: &stale_references,
		changed_decisions: changed,
		references: &input.index.references,
		material: input.material,
		candidates: &candidates,
	});
	timings.apply_store = apply_timer.elapsed();
	let semantic_timer = Instant::now();
	SemanticLinkage::new(input.material).enhance_changed(
		store.decisions_mut(),
		&input.index.references,
		&stale_references,
	);
	timings.semantic_enhance = semantic_timer.elapsed();
	let rebuild_timer = Instant::now();
	store.refresh_resolved_target_index(&stale_references);
	timings.rebuild_indexes = rebuild_timer.elapsed();
	timings
}

fn resolve_reference_decisions(
	input: &IncrementalLinkageInput<'_>,
	reference_indexes: &[usize],
	candidates: &CandidateCatalog<'_>,
) -> Vec<ReferenceLinkageDecision> {
	let resolver = LinkageResolver::new(input.material);
	let manifests = ManifestPolicy::build(input.material);
	indexes_to_references(input.index, reference_indexes)
		.par_iter()
		.map(|(reference_idx, reference)| {
			resolver.resolve_reference(*reference_idx, reference, candidates, &manifests)
		})
		.collect::<Vec<_>>()
}

fn reference_indexes_for(
	references: &[ReferenceRecord],
	reference_ids: &rustc_hash::FxHashSet<ReferenceId>,
) -> Vec<usize> {
	references
		.iter()
		.enumerate()
		.filter_map(|(idx, reference)| reference_ids.contains(&reference.id).then_some(idx))
		.collect()
}

fn indexes_to_references<'a>(
	index: &'a CodeIndex,
	reference_indexes: &[usize],
) -> Vec<(usize, &'a ReferenceRecord)> {
	reference_indexes
		.iter()
		.filter_map(|reference_idx| {
			index
				.references
				.get(*reference_idx)
				.map(|reference| (*reference_idx, reference))
		})
		.collect()
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
			store: LinkageStore::new(
				generation,
				index.generation,
				decisions,
				&index.references,
				self.material,
				&candidates,
			),
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
		let global_decision = manifests.evaluate_global_targets(&query, global_targets);
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

struct LinkageResolution {
	store: LinkageStore,
	timings: LinkageTimings,
}
