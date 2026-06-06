use std::time::{Duration, Instant};

use code_moniker_core::core::moniker::query::bare_callable_name;
use rayon::prelude::*;

use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::decision::ReferenceLinkageDecision;
use crate::linkage::delta::{
	LinkageRefreshImpact, LinkageRefreshShape, reference_id_remaps, symbol_id_remaps,
};
use crate::linkage::manifest::ManifestPolicy;
use crate::linkage::method_indexer::MethodIndexer;
use crate::linkage::metrics::LinkageMemoryMetrics;
use crate::linkage::ordinals::{ReferenceOrdinal, ReferenceSet};
use crate::linkage::planner::{LinkagePlanContext, execute_linkage_plan};
use crate::linkage::query::ReferenceLocations;
use crate::linkage::reference_resolver::ReferenceResolver;
use crate::linkage::resolver::{LinkageRefreshTimings, LocalLinkage, TimedLinkageRefresh};
use crate::linkage::semantic::SemanticLinkage;
use crate::linkage::store::{LinkageStore, LinkageStoreRefresh, reference_indexes};
use crate::snapshot::{
	CodeIndex, LinkageSnapshot, ReferenceId, ReferenceRecord, ResourceGeneration, WorkspaceResult,
};
use crate::source::CodeIndexMaterial;

pub(super) fn run_refresh_linkage_with_timings(
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
	if let Some(refresh) = refresh_symbol_only_without_linkage_work(FastRefreshInput {
		store: &mut linkage.store,
		previous,
		code_index,
		material: &material,
		impact: &refresh_impact,
		memory: linkage.memory,
		total_timer,
	}) {
		return Ok(refresh);
	}
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
	let refresh = run_incremental_refresh(
		store,
		indexer,
		&input,
		candidates,
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

struct FastRefreshInput<'a> {
	store: &'a mut Option<LinkageStore>,
	previous: &'a LinkageSnapshot,
	code_index: &'a CodeIndex,
	material: &'a CodeIndexMaterial,
	impact: &'a LinkageRefreshImpact,
	memory: LinkageMemoryMetrics,
	total_timer: Instant,
}

fn refresh_symbol_only_without_linkage_work(
	input: FastRefreshInput<'_>,
) -> Option<TimedLinkageRefresh> {
	let store = input.store.as_mut()?;
	let can_skip = match input.impact.shape() {
		LinkageRefreshShape::AdditiveSymbolsOnly(symbols) => {
			let added_keys = symbol_query_keys(input.material, symbols);
			!added_keys.is_empty() && !references_contain_any_key(store, &added_keys)
		}
		LinkageRefreshShape::RemovedSymbolsOnly(symbols) => {
			!resolved_references_contain_any_symbol(store, symbols)
		}
		_ => false,
	};
	can_skip.then(|| {
		refresh_without_linkage_work(
			store,
			input.previous,
			input.code_index,
			input.memory,
			input.total_timer,
		)
	})
}

fn refresh_without_linkage_work(
	store: &mut LinkageStore,
	previous: &LinkageSnapshot,
	code_index: &CodeIndex,
	memory: LinkageMemoryMetrics,
	total_timer: Instant,
) -> TimedLinkageRefresh {
	store.advance_index_generation(code_index.generation);
	let mut snapshot = previous.clone();
	snapshot.index_generation = code_index.generation;
	TimedLinkageRefresh {
		snapshot,
		timings: LinkageRefreshTimings {
			total: total_timer.elapsed(),
			..LinkageRefreshTimings::default()
		},
		memory,
	}
}

fn resolved_references_contain_any_symbol(
	store: &LinkageStore,
	symbols: &[crate::snapshot::SymbolId],
) -> bool {
	let Some(index) = &store.indexes.resolved_by_target_source else {
		return true;
	};
	symbols.iter().any(|symbol| {
		store
			.symbols
			.ordinal(symbol)
			.and_then(|ordinal| index.get_symbol(ordinal))
			.is_some_and(|references| !references.is_empty())
	})
}

fn references_contain_any_key(store: &LinkageStore, keys: &[Vec<u8>]) -> bool {
	keys.iter()
		.any(|key| store.indexes.references_by_name.contains_key(key))
}

fn symbol_query_keys(
	material: &CodeIndexMaterial,
	symbols: &[crate::snapshot::SymbolId],
) -> Vec<Vec<u8>> {
	let mut keys = Vec::new();
	for symbol in symbols {
		push_symbol_query_keys(material, symbol, &mut keys);
	}
	keys
}

fn push_symbol_query_keys(
	material: &CodeIndexMaterial,
	symbol: &crate::snapshot::SymbolId,
	keys: &mut Vec<Vec<u8>>,
) {
	let Some((file_idx, def_idx)) = material.identity.symbol_location(symbol) else {
		return;
	};
	let Some(file) = material.files.get(file_idx) else {
		return;
	};
	if def_idx >= file.graph.def_count() {
		return;
	}
	let def = file.graph.def_at(def_idx);
	if !def.call_name.is_empty() {
		push_unique_query_key(keys, def.call_name.to_vec());
	}
	if let Some(segment) = def.moniker.as_view().segments().last() {
		push_unique_query_key(keys, bare_callable_name(segment.name).to_vec());
	}
}

fn push_unique_query_key(keys: &mut Vec<Vec<u8>>, key: Vec<u8>) {
	if key.is_empty() || keys.iter().any(|existing| existing == &key) {
		return;
	}
	keys.push(key);
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
	let mut timings = LinkageRefreshTimings {
		candidate_index: candidate_index_elapsed,
		..LinkageRefreshTimings::default()
	};
	refresh_incremental_linkage(store, indexer, input, &candidates, &mut timings);
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
	indexer: &mut MethodIndexer,
	input: &IncrementalLinkageInput<'_>,
	candidates: &CandidateCatalog<'_>,
	timings: &mut LinkageRefreshTimings,
) {
	let reference_index_map = reference_indexes(&input.index.references);
	let plan_timer = Instant::now();
	store.rebase_reference_ordinals(reference_index_map, reference_id_remaps(&input.impact));
	store.ensure_resolved_target_index(input.material);
	let execution = execute_linkage_plan(LinkagePlanContext {
		store,
		references: &input.index.references,
		material: input.material,
		candidates,
		reference_indexes: &store.indexes.reference_indexes,
		impact: &input.impact,
	});
	timings.plan_invalidation = plan_timer.elapsed();
	timings.stale_refs = execution.stale_references().len() as usize;
	let changed_reference_indexes = stale_reference_indexes(execution.stale_references());
	timings.changed_refs = changed_reference_indexes.len();
	let locations = ReferenceLocations::from_material(input.material);
	let resolve_timer = Instant::now();
	let changed =
		resolve_reference_decisions(input, &changed_reference_indexes, candidates, &locations);
	timings.resolve_references = resolve_timer.elapsed();
	let apply_timer = Instant::now();
	store.apply_refresh(LinkageStoreRefresh {
		generation: input.generation,
		index_generation: input.index.generation,
		stale_references: execution.stale_references(),
		changed_decisions: changed,
		symbol_id_remaps: symbol_id_remaps(&input.impact),
		references: &input.index.references,
		material: input.material,
		candidates,
	});
	timings.apply_store = apply_timer.elapsed();
	if changed_reference_indexes.is_empty() {
		return;
	}
	let method_timer = Instant::now();
	let methods = indexer.reindex(input.material, candidates, execution.changed_files());
	timings.candidate_index += method_timer.elapsed();
	let semantic_timer = Instant::now();
	let stale_reference_ids =
		reference_ids_for_set(execution.stale_references(), &input.index.references);
	SemanticLinkage::new(input.material, methods, candidates, &locations).enhance_changed(
		store.decisions_mut(),
		&input.index.references,
		&stale_reference_ids,
	);
	timings.semantic_enhance = semantic_timer.elapsed();
	let rebuild_timer = Instant::now();
	store.refresh_resolved_target_index(execution.target_index_references(), input.material);
	timings.rebuild_indexes = rebuild_timer.elapsed();
}

fn resolve_reference_decisions(
	input: &IncrementalLinkageInput<'_>,
	reference_indexes: &[usize],
	candidates: &CandidateCatalog<'_>,
	locations: &ReferenceLocations,
) -> Vec<ReferenceLinkageDecision> {
	let resolver = ReferenceResolver::new(input.material);
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
