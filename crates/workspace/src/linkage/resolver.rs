use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::decision::{ReferenceLinkageDecision, ResolutionScope, UnknownReason};
use crate::linkage::gc::{LinkageGarbageCollector, LinkageRefreshImpact, changed_file_indexes};
use crate::linkage::manifest::ManifestPolicy;
use crate::linkage::method_indexer::MethodIndexer;
use crate::linkage::metrics::LinkageMemoryMetrics;
use crate::linkage::ordinals::{ReferenceOrdinal, ReferenceSet};
use crate::linkage::query::{LinkageQuery, ReferenceLocation, ReferenceLocations};
use crate::linkage::scope::{GlobalScopeResolver, LocalScopeResolver};
use crate::linkage::semantic::{MethodTable, SemanticLinkage};
use crate::linkage::store::{LinkageStore, LinkageStoreRefresh, reference_indexes};
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
	pub semantic_enhance: Duration,
	pub store_index: Duration,
	pub project_snapshot: Duration,
	pub total: Duration,
}

pub struct TimedLinkageSnapshot {
	pub snapshot: LinkageSnapshot,
	pub timings: LinkageTimings,
	pub memory: LinkageMemoryMetrics,
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
	pub memory: LinkageMemoryMetrics,
}

pub struct LocalLinkage {
	cache: LocalResourceCache,
	store: Option<LinkageStore>,
	method_indexer: Option<MethodIndexer>,
}

impl LocalLinkage {
	pub fn new(cache: LocalResourceCache) -> Self {
		Self {
			cache,
			store: None,
			method_indexer: None,
		}
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
		run_refresh_linkage_with_timings(self, previous, code_index, refresh_impact)
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
		run_full_linkage_with_timings(self, index)
	}
}

fn run_refresh_linkage_with_timings(
	linkage: &mut LocalLinkage,
	previous: &LinkageSnapshot,
	code_index: &CodeIndex,
	refresh_impact: LinkageRefreshImpact,
) -> WorkspaceResult<TimedLinkageRefresh> {
	let total_timer = Instant::now();
	if refresh_impact.is_empty() {
		return Ok(refresh_empty_linkage(
			&mut linkage.store,
			previous,
			code_index,
			total_timer,
		));
	}
	let material = linkage.linkage_material(code_index)?;
	let generation = linkage.cache.next_generation();
	let candidate_timer = Instant::now();
	let candidates = CandidateCatalog::new(&material);
	let mut candidate_index = candidate_timer.elapsed();
	if linkage.store.is_none() {
		linkage.store = Some(LinkageStore::from_snapshot(
			previous,
			&code_index.references,
			&material,
			&candidates,
		));
	}
	let store = linkage
		.store
		.as_mut()
		.expect("linkage store is initialized before refresh");
	let method_timer = Instant::now();
	let indexer = linkage
		.method_indexer
		.get_or_insert_with(|| MethodIndexer::new(&material, &candidates));
	candidate_index += method_timer.elapsed();
	let input = IncrementalLinkageInput {
		index: code_index,
		impact: refresh_impact,
		material: &material,
		generation,
	};
	Ok(run_incremental_refresh(
		store,
		indexer,
		&input,
		candidates,
		candidate_index,
		total_timer,
	))
}

fn run_full_linkage_with_timings(
	linkage: &mut LocalLinkage,
	index: &CodeIndex,
) -> WorkspaceResult<TimedLinkageSnapshot> {
	let total_timer = Instant::now();
	let material = linkage.linkage_material(index)?;
	let generation = linkage.cache.next_generation();
	let candidate_timer = Instant::now();
	let candidates = CandidateCatalog::new(&material);
	let mut candidate_index = candidate_timer.elapsed();
	let method_timer = Instant::now();
	let method_indexer = MethodIndexer::new(&material, &candidates);
	candidate_index += method_timer.elapsed();
	let LinkageResolution { store, mut timings } = resolve_full_linkage(
		&material,
		index,
		generation,
		method_indexer.methods(),
		candidates,
		candidate_index,
	);
	let report_timer = Instant::now();
	let snapshot = store.project_snapshot(&index.references, &material.identity);
	let memory = store.memory_metrics();
	timings.project_snapshot = report_timer.elapsed();
	timings.total = total_timer.elapsed();
	linkage.store = Some(store);
	linkage.method_indexer = Some(method_indexer);
	Ok(TimedLinkageSnapshot {
		snapshot,
		timings,
		memory,
	})
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
		memory: store
			.as_ref()
			.map(LinkageStore::memory_metrics)
			.unwrap_or_default(),
	}
}

struct IncrementalLinkageInput<'a> {
	index: &'a CodeIndex,
	impact: LinkageRefreshImpact,
	material: &'a CodeIndexMaterial,
	generation: ResourceGeneration,
}

fn run_incremental_refresh(
	store: &mut LinkageStore,
	indexer: &mut MethodIndexer,
	input: &IncrementalLinkageInput<'_>,
	candidates: CandidateCatalog<'_>,
	candidate_index_elapsed: Duration,
	total_timer: Instant,
) -> TimedLinkageRefresh {
	let mut timings = LinkageRefreshTimings::default();
	let changed_files = changed_file_indexes(input.material, &input.impact);
	let method_timer = Instant::now();
	let methods = indexer.reindex(input.material, &candidates, &changed_files);
	timings.candidate_index = candidate_index_elapsed + method_timer.elapsed();
	refresh_incremental_linkage(store, methods, input, &candidates, &mut timings);
	let project_timer = Instant::now();
	let snapshot = store.project_snapshot(&input.index.references, &input.material.identity);
	let memory = store.memory_metrics();
	timings.project_snapshot = project_timer.elapsed();
	timings.total = total_timer.elapsed();
	TimedLinkageRefresh {
		snapshot,
		timings,
		memory,
	}
}

fn refresh_incremental_linkage(
	store: &mut LinkageStore,
	methods: &MethodTable,
	input: &IncrementalLinkageInput<'_>,
	candidates: &CandidateCatalog<'_>,
	timings: &mut LinkageRefreshTimings,
) {
	let reference_index_map = reference_indexes(&input.index.references);
	let gc_timer = Instant::now();
	store.rebase_reference_ordinals(reference_index_map, input.impact.reference_id_remaps());
	store.ensure_resolved_target_index(input.material);
	let stale_references = LinkageGarbageCollector::new(
		store,
		&input.index.references,
		input.material,
		candidates,
		&store.indexes.reference_indexes,
		&input.impact,
	)
	.collect();
	timings.garbage_collect = gc_timer.elapsed();
	timings.stale_refs = stale_references.len() as usize;
	let changed_reference_indexes = stale_reference_indexes(&stale_references);
	timings.changed_refs = changed_reference_indexes.len();
	let target_index_references = target_index_refresh_references(
		&stale_references,
		input.impact.reference_id_remaps(),
		&store.indexes.reference_indexes,
	);
	let locations = ReferenceLocations::from_material(input.material);
	let resolve_timer = Instant::now();
	let changed =
		resolve_reference_decisions(input, &changed_reference_indexes, candidates, &locations);
	timings.resolve_references = resolve_timer.elapsed();
	let apply_timer = Instant::now();
	store.apply_refresh(LinkageStoreRefresh {
		generation: input.generation,
		index_generation: input.index.generation,
		stale_references: &stale_references,
		changed_decisions: changed,
		symbol_id_remaps: input.impact.symbol_id_remaps(),
		references: &input.index.references,
		material: input.material,
		candidates,
	});
	timings.apply_store = apply_timer.elapsed();
	let semantic_timer = Instant::now();
	let stale_reference_ids = reference_ids_for_set(&stale_references, &input.index.references);
	SemanticLinkage::new(input.material, methods, candidates, &locations).enhance_changed(
		store.decisions_mut(),
		&input.index.references,
		&stale_reference_ids,
	);
	timings.semantic_enhance = semantic_timer.elapsed();
	let rebuild_timer = Instant::now();
	store.refresh_resolved_target_index(&target_index_references, input.material);
	timings.rebuild_indexes = rebuild_timer.elapsed();
}

fn resolve_reference_decisions(
	input: &IncrementalLinkageInput<'_>,
	reference_indexes: &[usize],
	candidates: &CandidateCatalog<'_>,
	locations: &ReferenceLocations,
) -> Vec<ReferenceLinkageDecision> {
	let resolver = LinkageResolver::new(input.material);
	let manifests = ManifestPolicy::build(input.material);
	indexes_to_references(input.index, reference_indexes)
		.par_iter()
		.map(|(reference_idx, reference)| {
			resolver.resolve_reference(
				*reference_idx,
				reference,
				locations.get(*reference_idx),
				candidates,
				&manifests,
			)
		})
		.collect::<Vec<_>>()
}

fn stale_reference_indexes(stale_references: &ReferenceSet) -> Vec<usize> {
	stale_references
		.iter()
		.map(ReferenceOrdinal::index)
		.collect()
}

fn target_index_refresh_references(
	stale_references: &ReferenceSet,
	reference_id_remaps: &[(ReferenceId, ReferenceId)],
	reference_indexes: &rustc_hash::FxHashMap<ReferenceId, ReferenceOrdinal>,
) -> ReferenceSet {
	let mut references = stale_references.clone();
	for (_, next_reference) in reference_id_remaps {
		if let Some(reference_idx) = reference_indexes.get(next_reference) {
			references.insert(*reference_idx);
		}
	}
	references
}

fn reference_ids_for_set(
	references: &ReferenceSet,
	records: &[ReferenceRecord],
) -> rustc_hash::FxHashSet<ReferenceId> {
	references
		.iter()
		.filter_map(|reference| records.get(reference.index()))
		.map(|reference| reference.id.clone())
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

	fn resolve_reference(
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

fn resolve_full_linkage(
	material: &CodeIndexMaterial,
	index: &CodeIndex,
	generation: ResourceGeneration,
	methods: &MethodTable,
	candidates: CandidateCatalog<'_>,
	candidate_index_elapsed: Duration,
) -> LinkageResolution {
	let resolver = LinkageResolver::new(material);
	let mut timings = LinkageTimings {
		candidate_index: candidate_index_elapsed,
		..LinkageTimings::default()
	};
	let manifest_timer = Instant::now();
	let manifests = ManifestPolicy::build(material);
	timings.manifest_policy = manifest_timer.elapsed();
	let resolve_timer = Instant::now();
	let locations = ReferenceLocations::from_material(material);
	let mut decisions = index
		.references
		.par_iter()
		.enumerate()
		.map(|(reference_idx, reference)| {
			resolver.resolve_reference(
				reference_idx,
				reference,
				locations.get(reference_idx),
				&candidates,
				&manifests,
			)
		})
		.collect::<Vec<_>>();
	timings.resolve_references = resolve_timer.elapsed();
	let semantic_timer = Instant::now();
	SemanticLinkage::new(material, methods, &candidates, &locations)
		.enhance(&mut decisions, &index.references);
	timings.semantic_enhance = semantic_timer.elapsed();
	let store_timer = Instant::now();
	let store = LinkageStore::new(
		generation,
		index.generation,
		decisions,
		&index.references,
		material,
		&candidates,
	);
	timings.store_index = store_timer.elapsed();
	LinkageResolution { store, timings }
}

struct LinkageResolution {
	store: LinkageStore,
	timings: LinkageTimings,
}
