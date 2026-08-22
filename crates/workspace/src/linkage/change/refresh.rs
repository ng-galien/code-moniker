use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::linkage::binding::LinkageMemoryMetrics;
use crate::linkage::binding::ReferenceLinkageDecision;
use crate::linkage::binding::{
	LinkageStore, LinkageStoreRefresh, insert_reference_ordinals, reference_indexes,
};
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::ReferenceLocations;
use crate::linkage::catalog::{ReferenceOrdinal, ReferenceSet};
use crate::linkage::change::{BindingReadModel, EditedGraph, RebindScope};
use crate::linkage::change::{LinkageRefreshImpact, SymbolDelta, changes_c_include_topology};
use crate::linkage::resolve::BindingForwards;
use crate::linkage::resolve::LinkagePolicies;
use crate::linkage::resolve::LinkageRefiner;
use crate::linkage::resolve::ManifestPolicy;
use crate::linkage::resolve::MethodIndexer;
use crate::linkage::resolve::ReferenceResolver;
use crate::linkage::resolve::WorkspacePackageIndex;
use crate::linkage::resolve::run_full_linkage_with_timings;
use crate::linkage::source_groups::SourceGroupPolicy;
use crate::linkage::{LinkageRefreshTimings, LocalLinkage, TimedLinkageRefresh};
use crate::snapshot::{
	CodeIndex, LinkageSnapshot, RecordTable, ReferenceId, ReferenceRecord, ResourceGeneration,
	WorkspaceResult,
};
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) fn run_refresh_linkage_with_timings(
	linkage: &mut LocalLinkage,
	previous: &LinkageSnapshot,
	code_index: &CodeIndex,
	refresh_impact: LinkageRefreshImpact,
) -> WorkspaceResult<TimedLinkageRefresh> {
	let total_timer = Instant::now();
	if refresh_impact.is_empty() {
		let memory = linkage.memory;
		return Ok(refresh_empty_linkage(
			&mut linkage.store,
			previous,
			code_index,
			memory,
			total_timer,
		));
	}
	let material = linkage.linkage_material(code_index)?;
	if changes_c_include_topology(&refresh_impact, &material) {
		let full = run_full_linkage_with_timings(linkage, code_index)?;
		return Ok(TimedLinkageRefresh {
			snapshot: full.snapshot,
			timings: LinkageRefreshTimings {
				candidate_index: full.timings.candidate_index,
				plan_invalidation: full.timings.manifest_policy,
				resolve_references: full.timings.resolve_references,
				semantic_refinement: full.timings.semantic_refinement,
				rebuild_indexes: full.timings.store_index,
				project_snapshot: full.timings.project_snapshot,
				total: full.timings.total,
				stale_refs: code_index.references.len(),
				changed_refs: code_index.references.len(),
				..LinkageRefreshTimings::default()
			},
			memory: full.memory,
		});
	}
	let generation = linkage.cache.next_generation();
	let candidate_timer = Instant::now();
	let candidates = match linkage.candidates.as_mut() {
		Some(candidates) => {
			candidates.refresh_files(
				&material,
				std::sync::Arc::clone(code_index.inventory.catalog()),
			);
			candidates
		}
		None => linkage.candidates.get_or_insert_with(|| {
			CandidateCatalog::new(
				&material,
				std::sync::Arc::clone(code_index.inventory.catalog()),
			)
		}),
	};
	let candidates = &*candidates;
	let mut candidate_index = candidate_timer.elapsed();
	if linkage.store.is_none() {
		linkage.store = Some(LinkageStore::from_snapshot(
			previous,
			&code_index.references,
			&material,
			candidates,
		));
	}
	let Some(store) = linkage.store.as_mut() else {
		panic!("linkage store is initialized before refresh");
	};
	let method_timer = Instant::now();
	let indexer = linkage
		.method_indexer
		.get_or_insert_with(|| MethodIndexer::new(&material, candidates));
	candidate_index += method_timer.elapsed();
	let input = IncrementalLinkageInput {
		index: code_index,
		impact: refresh_impact,
		material: &material,
		generation,
	};
	let refresh = run_incremental_refresh(
		RefreshExecution {
			store,
			indexer,
			candidates,
			previous,
		},
		&input,
		candidate_index,
		total_timer,
	);
	linkage.memory = refresh.memory;
	Ok(refresh)
}

fn refresh_empty_linkage(
	store: &mut Option<LinkageStore>,
	previous: &LinkageSnapshot,
	code_index: &CodeIndex,
	memory: LinkageMemoryMetrics,
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
		memory,
	}
}

struct IncrementalLinkageInput<'a> {
	index: &'a CodeIndex,
	impact: LinkageRefreshImpact,
	material: &'a CodeIndexMaterial,
	generation: ResourceGeneration,
}

struct RefreshExecution<'a> {
	store: &'a mut LinkageStore,
	indexer: &'a mut MethodIndexer,
	candidates: &'a CandidateCatalog,
	previous: &'a LinkageSnapshot,
}

fn run_incremental_refresh(
	execution: RefreshExecution<'_>,
	input: &IncrementalLinkageInput<'_>,
	candidate_index_elapsed: Duration,
	total_timer: Instant,
) -> TimedLinkageRefresh {
	let RefreshExecution {
		store,
		indexer,
		candidates,
		previous,
	} = execution;
	let mut timings = LinkageRefreshTimings {
		candidate_index: candidate_index_elapsed,
		..LinkageRefreshTimings::default()
	};
	let decisions_unchanged =
		refresh_incremental_linkage(store, indexer, input, candidates, &mut timings);
	let project_timer = Instant::now();
	let snapshot = if decisions_unchanged {
		let mut snapshot = previous.clone();
		snapshot.generation = input.generation;
		snapshot.index_generation = input.index.generation;
		snapshot
	} else {
		store.project_snapshot(
			&input.index.references,
			input.material,
			candidates.symbol_catalog(),
		)
	};
	let memory = store.memory_metrics(candidates.symbols());
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
	indexer: &mut MethodIndexer,
	input: &IncrementalLinkageInput<'_>,
	candidates: &CandidateCatalog,
	timings: &mut LinkageRefreshTimings,
) -> bool {
	let plan_timer = Instant::now();
	let positions_stable = input.impact.references().id_remaps().is_empty()
		&& input.impact.references().removed_ids().is_empty()
		&& store.indexes.reference_indexes.len() == input.index.references.len();
	if positions_stable {
		insert_reference_ordinals(
			store,
			input.impact.references().changed_ids(),
			&input.index.references,
			input.material,
		);
	} else {
		store.rebase_reference_ordinals(
			reference_indexes(&input.index.references),
			input.impact.references().id_remaps(),
			input.impact.references().removed_ids(),
		);
	}
	store.ensure_resolved_target_index(input.material, candidates.symbols());
	let RebindScope {
		stale_references,
		target_index_references,
		changed_files,
	} = RebindScope::plan(
		BindingReadModel {
			store,
			inventory: &input.index.inventory,
			reference_indexes: &store.indexes.reference_indexes,
		},
		EditedGraph {
			references: &input.index.references,
			material: input.material,
			candidates,
		},
		&input.impact,
	);
	timings.plan_invalidation = plan_timer.elapsed();
	timings.stale_refs = stale_references.len() as usize;
	let changed_reference_indexes = stale_reference_indexes(&stale_references);
	timings.changed_refs = changed_reference_indexes.len();
	let locations = (!changed_reference_indexes.is_empty())
		.then(|| ReferenceLocations::from_material(input.material));
	let refresh_policies = locations
		.as_ref()
		.map(|_| RefreshPolicies::build(input.material));
	let resolve_timer = Instant::now();
	let changed = match (&locations, &refresh_policies) {
		(Some(locations), Some(policies)) => resolve_reference_decisions(
			input,
			&changed_reference_indexes,
			candidates,
			locations,
			policies,
		),
		(None, None) => Vec::new(),
		_ => unreachable!("locations and policies are built together"),
	};
	timings.resolve_references = resolve_timer.elapsed();
	let apply_timer = Instant::now();
	store.apply_refresh(LinkageStoreRefresh {
		generation: input.generation,
		index_generation: input.index.generation,
		stale_references: &stale_references,
		changed_decisions: changed,
		references: &input.index.references,
		material: input.material,
	});
	timings.apply_store = apply_timer.elapsed();
	if changed_reference_indexes.is_empty() {
		let symbol_ids_stable = matches!(
			input.impact.definitions(),
			SymbolDelta::Unchanged | SymbolDelta::AdditiveOnly { .. }
		);
		return positions_stable && symbol_ids_stable && stale_references.is_empty();
	}
	let method_timer = Instant::now();
	let methods = indexer.reindex(input.material, candidates, &changed_files);
	timings.candidate_index += method_timer.elapsed();
	let refinement_timer = Instant::now();
	let stale_reference_ids = reference_ids_for_set(&stale_references, &input.index.references);
	let locations = locations.unwrap_or_else(|| ReferenceLocations::from_material(input.material));
	let Some(refresh_policies) = refresh_policies.as_ref() else {
		unreachable!("changed references always build refresh policies");
	};
	LinkageRefiner::new(
		input.material,
		methods,
		candidates,
		&locations,
		crate::linkage::resolve::RefinementPolicies::new(
			&refresh_policies.source_groups,
			&refresh_policies.packages,
			&refresh_policies.manifests,
		),
	)
	.refine_changed(
		store.decisions_mut(),
		&input.index.references,
		&stale_reference_ids,
	);
	timings.semantic_refinement = refinement_timer.elapsed();
	let rebuild_timer = Instant::now();
	store.refresh_resolved_target_index(
		&target_index_references,
		input.material,
		candidates.symbols(),
	);
	timings.rebuild_indexes = rebuild_timer.elapsed();
	false
}

struct RefreshPolicies {
	manifests: ManifestPolicy,
	source_groups: SourceGroupPolicy,
	packages: WorkspacePackageIndex,
}

impl RefreshPolicies {
	fn build(material: &CodeIndexMaterial) -> Self {
		Self {
			manifests: ManifestPolicy::build(material),
			source_groups: SourceGroupPolicy::build(material),
			packages: WorkspacePackageIndex::build(material),
		}
	}
}

fn resolve_reference_decisions(
	input: &IncrementalLinkageInput<'_>,
	reference_indexes: &[usize],
	candidates: &CandidateCatalog,
	locations: &ReferenceLocations,
	refresh_policies: &RefreshPolicies,
) -> Vec<ReferenceLinkageDecision> {
	let forwards = BindingForwards::build(input.material, &refresh_policies.manifests);
	let java_on_demand = crate::linkage::resolve::JavaOnDemandImports::build(input.material);
	let policies = LinkagePolicies {
		candidates,
		manifests: &refresh_policies.manifests,
		source_groups: &refresh_policies.source_groups,
		packages: &refresh_policies.packages,
		forwards: &forwards,
		java_on_demand: &java_on_demand,
	};
	let resolver = ReferenceResolver::new(input.material, &policies);
	indexes_to_references(input.index, reference_indexes)
		.par_iter()
		.map(|(reference_idx, reference)| {
			resolver.resolve_reference(*reference_idx, reference, locations.get(*reference_idx))
		})
		.collect::<Vec<_>>()
}

fn stale_reference_indexes(stale_references: &ReferenceSet) -> Vec<usize> {
	stale_references
		.iter()
		.map(ReferenceOrdinal::index)
		.collect()
}

fn reference_ids_for_set(
	references: &ReferenceSet,
	records: &RecordTable<ReferenceRecord>,
) -> rustc_hash::FxHashSet<ReferenceId> {
	references
		.iter()
		.filter_map(|reference| records.get(reference.index()))
		.map(|reference| reference.id)
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
