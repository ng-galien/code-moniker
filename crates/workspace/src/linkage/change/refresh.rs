use std::time::{Duration, Instant};

use code_moniker_core::core::moniker::query::bare_callable_name;
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
use crate::linkage::change::{LinkageRefreshImpact, LinkageRefreshShape, SymbolDelta};
use crate::linkage::resolve::ManifestPolicy;
use crate::linkage::resolve::MethodIndexer;
use crate::linkage::resolve::ReferenceResolver;
use crate::linkage::resolve::SemanticLinkage;
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
	if let Some(refresh) = refresh_symbol_only_without_linkage_work(
		FastRefreshInput {
			store: &mut linkage.store,
			previous,
			code_index,
			material: &material,
			impact: &refresh_impact,
			memory: linkage.memory,
			total_timer,
		},
		linkage.candidates.as_ref(),
	) {
		return Ok(refresh);
	}
	let generation = linkage.cache.next_generation();
	let candidate_timer = Instant::now();
	let candidates = match linkage.candidates.as_mut() {
		Some(candidates) => {
			candidates.refresh_files(&material);
			candidates
		}
		None => linkage
			.candidates
			.get_or_insert_with(|| CandidateCatalog::new(&material)),
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
	candidates: Option<&CandidateCatalog>,
) -> Option<TimedLinkageRefresh> {
	let store = input.store.as_mut()?;
	let can_skip = match input.impact.shape() {
		LinkageRefreshShape::AdditiveSymbolsOnly(symbols) => {
			let added_keys = symbol_query_keys(input.material, symbols);
			!added_keys.is_empty() && !references_contain_any_key(store, &added_keys)
		}
		LinkageRefreshShape::RemovedSymbolsOnly(symbols) => candidates.is_some_and(|candidates| {
			!resolved_references_contain_any_symbol(store, candidates.symbols(), symbols)
		}),
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
	catalog: &crate::linkage::catalog::SymbolOrdinalCatalog,
	symbols: &[crate::snapshot::SymbolId],
) -> bool {
	let Some(index) = &store.indexes.resolved_by_target_source else {
		return true;
	};
	symbols.iter().any(|symbol| {
		catalog
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
			&input.material.identity,
			candidates.symbols(),
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
	let execution = RebindScope::plan(
		BindingReadModel {
			store,
			symbols: candidates.symbols(),
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
	timings.stale_refs = execution.stale_references().len() as usize;
	let changed_reference_indexes = stale_reference_indexes(execution.stale_references());
	timings.changed_refs = changed_reference_indexes.len();
	let locations = (!changed_reference_indexes.is_empty())
		.then(|| ReferenceLocations::from_material(input.material));
	let resolve_timer = Instant::now();
	let changed = match &locations {
		Some(locations) => {
			resolve_reference_decisions(input, &changed_reference_indexes, candidates, locations)
		}
		None => Vec::new(),
	};
	timings.resolve_references = resolve_timer.elapsed();
	let apply_timer = Instant::now();
	store.apply_refresh(LinkageStoreRefresh {
		generation: input.generation,
		index_generation: input.index.generation,
		stale_references: execution.stale_references(),
		changed_decisions: changed,
		references: &input.index.references,
		material: input.material,
		candidates,
	});
	timings.apply_store = apply_timer.elapsed();
	if changed_reference_indexes.is_empty() {
		let symbol_ids_stable = matches!(
			input.impact.definitions(),
			SymbolDelta::Unchanged | SymbolDelta::AdditiveOnly { .. }
		);
		return positions_stable && symbol_ids_stable && execution.stale_references().is_empty();
	}
	let method_timer = Instant::now();
	let methods = indexer.reindex(input.material, candidates, execution.changed_files());
	timings.candidate_index += method_timer.elapsed();
	let semantic_timer = Instant::now();
	let stale_reference_ids =
		reference_ids_for_set(execution.stale_references(), &input.index.references);
	let locations = locations.unwrap_or_else(|| ReferenceLocations::from_material(input.material));
	SemanticLinkage::new(input.material, methods, candidates, &locations).enhance_changed(
		store.decisions_mut(),
		&input.index.references,
		&stale_reference_ids,
	);
	timings.semantic_enhance = semantic_timer.elapsed();
	let rebuild_timer = Instant::now();
	store.refresh_resolved_target_index(
		execution.target_index_references(),
		input.material,
		candidates.symbols(),
	);
	timings.rebuild_indexes = rebuild_timer.elapsed();
	false
}

fn resolve_reference_decisions(
	input: &IncrementalLinkageInput<'_>,
	reference_indexes: &[usize],
	candidates: &CandidateCatalog,
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
	records: &RecordTable<ReferenceRecord>,
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
