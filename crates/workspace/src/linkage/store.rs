use std::collections::BTreeMap;
use std::hash::Hash;

use crate::linkage::candidate::{CandidateCatalog, query_keys};
use crate::linkage::decision::{ReferenceLinkageDecision, UnknownReason};
use crate::linkage::metrics::LinkageMemoryMetrics;
use crate::linkage::ordinals::{
	ReferenceOrdinal, ReferenceSet, SymbolOrdinal, SymbolOrdinalCatalog, SymbolSet,
};
use crate::linkage::query::LinkageQuery;
use crate::snapshot::{
	LinkageSnapshot, ReferenceId, ReferenceRecord, ResourceGeneration, SourceId, SymbolId,
};
use crate::source::{CodeIndexMaterial, LocalIdentityResolver};
use code_moniker_core::core::uri::{UriConfig, from_uri};
use rayon::prelude::*;
use rustc_hash::FxHashMap;

#[derive(Clone)]
pub(super) struct LinkageStore {
	generation: ResourceGeneration,
	index_generation: ResourceGeneration,
	decisions: Vec<ReferenceLinkageDecision>,
	pub(super) symbols: SymbolOrdinalCatalog,
	pub(super) indexes: LinkageStoreIndexes,
}

pub(super) struct LinkageStoreRefresh<'a> {
	pub(super) generation: ResourceGeneration,
	pub(super) index_generation: ResourceGeneration,
	pub(super) stale_references: &'a ReferenceSet,
	pub(super) changed_decisions: Vec<ReferenceLinkageDecision>,
	pub(super) symbol_id_remaps: &'a [(SymbolId, SymbolId)],
	pub(super) references: &'a [ReferenceRecord],
	pub(super) material: &'a CodeIndexMaterial,
	pub(super) candidates: &'a CandidateCatalog<'a>,
}

impl LinkageStore {
	pub(super) fn new(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		decisions: Vec<ReferenceLinkageDecision>,
		references: &[ReferenceRecord],
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) -> Self {
		let symbols = candidates.symbols().clone();
		let mut indexes = LinkageStoreIndexes::new(references, material);
		indexes.rebuild_resolved_target_indexes(ResolvedTargetSourceBuild {
			decisions: &decisions,
			material,
			symbols: &symbols,
		});
		Self {
			generation,
			index_generation,
			decisions,
			symbols,
			indexes,
		}
	}

	pub(super) fn from_snapshot(
		snapshot: &LinkageSnapshot,
		references: &[ReferenceRecord],
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) -> Self {
		Self::new(
			snapshot.generation,
			snapshot.index_generation,
			decisions_from_snapshot(snapshot, references, material, candidates),
			references,
			material,
			candidates,
		)
	}

	pub(super) fn project_snapshot(
		&self,
		references: &[ReferenceRecord],
		identity: &LocalIdentityResolver,
	) -> LinkageSnapshot {
		LinkageSnapshot::from_report(
			crate::linkage::decision::project_decisions(
				&self.decisions,
				references,
				identity,
				&self.symbols,
			)
			.into_report(self.generation, self.index_generation),
		)
	}

	pub(super) fn advance_index_generation(&mut self, index_generation: ResourceGeneration) {
		self.index_generation = index_generation;
	}

	pub(super) fn apply_refresh(&mut self, refresh: LinkageStoreRefresh<'_>) {
		apply_store_refresh(self, refresh);
	}

	pub(super) fn rebase_reference_ordinals(
		&mut self,
		next_reference_indexes: FxHashMap<ReferenceId, ReferenceOrdinal>,
		reference_id_remaps: &[(ReferenceId, ReferenceId)],
	) {
		rebase_store_reference_ordinals(self, next_reference_indexes, reference_id_remaps);
	}

	pub(super) fn missing_resolved_references(
		&self,
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) -> Vec<ReferenceId> {
		missing_resolved_references(self, material, candidates)
	}

	pub(super) fn decisions_mut(&mut self) -> &mut [ReferenceLinkageDecision] {
		&mut self.decisions
	}

	pub(super) fn memory_metrics(&self) -> LinkageMemoryMetrics {
		store_memory_metrics(self)
	}

	pub(super) fn refresh_resolved_target_index(
		&mut self,
		references: &ReferenceSet,
		material: &CodeIndexMaterial,
	) {
		refresh_resolved_target_index(self, references, material);
	}

	pub(super) fn ensure_resolved_target_index(&mut self, material: &CodeIndexMaterial) {
		if self.indexes.resolved_by_target_source.is_some() {
			return;
		}
		self.indexes
			.rebuild_resolved_target_indexes(ResolvedTargetSourceBuild {
				decisions: &self.decisions,
				material,
				symbols: &self.symbols,
			});
	}
}

fn apply_store_refresh(store: &mut LinkageStore, refresh: LinkageStoreRefresh<'_>) {
	let LinkageStoreRefresh {
		generation,
		index_generation,
		stale_references,
		changed_decisions,
		symbol_id_remaps,
		references,
		material,
		candidates,
	} = refresh;
	store.generation = generation;
	store.index_generation = index_generation;
	store.indexes.remove_stale_references(stale_references);
	remove_stale_decisions(store, stale_references);
	remap_symbol_ordinals(store, candidates.symbols(), symbol_id_remaps);
	add_changed_decisions(
		store,
		ChangedDecisionBatch {
			decisions: changed_decisions,
			references,
			material,
		},
	);
}

struct ChangedDecisionBatch<'a> {
	decisions: Vec<ReferenceLinkageDecision>,
	references: &'a [ReferenceRecord],
	material: &'a CodeIndexMaterial,
}

fn missing_resolved_references(
	store: &LinkageStore,
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog<'_>,
) -> Vec<ReferenceId> {
	store
		.decisions
		.iter()
		.filter(|decision| {
			!store
				.indexes
				.reference_indexes
				.contains_key(decision.reference())
				|| decision.resolved_targets().is_some_and(|targets| {
					targets.iter().any(|target| {
						resolved_target_missing_or_retargeted(store, material, candidates, target)
					})
				})
		})
		.map(|decision| decision.reference().clone())
		.collect()
}

fn resolved_target_missing_or_retargeted(
	store: &LinkageStore,
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog<'_>,
	target: SymbolOrdinal,
) -> bool {
	let Some(expected_identity) = store.symbols.identity(target) else {
		return true;
	};
	if candidates
		.symbols()
		.ordinal_by_identity(expected_identity)
		.is_some()
	{
		return false;
	}
	let Some(id) = store.symbols.id(target) else {
		return true;
	};
	let Some(current_moniker) = material.symbol_moniker(id) else {
		return true;
	};
	material.identity.moniker_uri(current_moniker) != expected_identity
}

fn rebase_store_reference_ordinals(
	store: &mut LinkageStore,
	next_reference_indexes: FxHashMap<ReferenceId, ReferenceOrdinal>,
	reference_id_remaps: &[(ReferenceId, ReferenceId)],
) {
	let rebase = ReferenceOrdinalRebase::new(
		&store.indexes.reference_indexes,
		&next_reference_indexes,
		reference_id_remaps,
	);
	store.indexes.rebase_reference_ordinals(&rebase);
	rebase_decision_references(store, &next_reference_indexes, reference_id_remaps);
	store.indexes.reference_indexes = next_reference_indexes;
}

struct ReferenceOrdinalRebase {
	next_by_old: Vec<Option<ReferenceOrdinal>>,
}

impl ReferenceOrdinalRebase {
	fn new(
		previous: &FxHashMap<ReferenceId, ReferenceOrdinal>,
		next: &FxHashMap<ReferenceId, ReferenceOrdinal>,
		reference_id_remaps: &[(ReferenceId, ReferenceId)],
	) -> Self {
		let max_old = previous
			.values()
			.map(|reference| reference.index())
			.max()
			.unwrap_or(0);
		let mut next_by_old = vec![None; max_old + 1];
		let reference_id_remaps = reference_id_remaps
			.iter()
			.cloned()
			.collect::<FxHashMap<ReferenceId, ReferenceId>>();
		for (reference, previous_ordinal) in previous {
			let next_reference = reference_id_remaps.get(reference).unwrap_or(reference);
			if let Some(next_ordinal) = next.get(next_reference) {
				next_by_old[previous_ordinal.index()] = Some(*next_ordinal);
			}
		}
		Self { next_by_old }
	}

	fn map(&self, previous: ReferenceOrdinal) -> Option<ReferenceOrdinal> {
		self.next_by_old.get(previous.index()).copied().flatten()
	}
}

fn rebase_decision_references(
	store: &mut LinkageStore,
	next_reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
	reference_id_remaps: &[(ReferenceId, ReferenceId)],
) {
	let reference_id_remaps = reference_id_remaps
		.iter()
		.cloned()
		.collect::<FxHashMap<ReferenceId, ReferenceId>>();
	store.decisions.retain_mut(|decision| {
		let current_reference = decision.reference().clone();
		let next_reference = reference_id_remaps
			.get(&current_reference)
			.unwrap_or(&current_reference);
		let Some(next_reference_idx) = next_reference_indexes.get(next_reference) else {
			return false;
		};
		if next_reference == &current_reference {
			decision.set_reference_idx(next_reference_idx.index());
		} else {
			decision.set_reference(next_reference.clone(), next_reference_idx.index());
		}
		true
	});
}

fn remove_stale_decisions(store: &mut LinkageStore, stale_references: &ReferenceSet) {
	let reference_indexes = &store.indexes.reference_indexes;
	store.decisions.retain_mut(|decision| {
		if let Some(reference_idx) = reference_indexes.get(decision.reference()) {
			if stale_references.contains(*reference_idx) {
				return false;
			}
			decision.set_reference_idx(reference_idx.index());
			return true;
		}
		false
	});
}

fn remap_symbol_ordinals(
	store: &mut LinkageStore,
	next: &SymbolOrdinalCatalog,
	symbol_id_remaps: &[(SymbolId, SymbolId)],
) {
	if store.symbols.has_same_order(next) {
		return;
	}
	let previous = &store.symbols;
	let symbol_id_remaps = symbol_id_remaps
		.iter()
		.cloned()
		.collect::<FxHashMap<SymbolId, SymbolId>>();
	store
		.decisions
		.retain_mut(|decision| decision.remap_resolved_targets(previous, next, &symbol_id_remaps));
	store
		.indexes
		.rebase_symbol_ordinals(previous, next, &symbol_id_remaps);
	store.symbols = next.clone();
}

fn add_changed_decisions(store: &mut LinkageStore, batch: ChangedDecisionBatch<'_>) {
	for decision in batch.decisions {
		let Some(reference) = batch.references.get(decision.reference_idx()) else {
			continue;
		};
		add_reference_indexes(&mut store.indexes, reference, batch.material);
		store.decisions.push(decision);
	}
}

fn add_reference_indexes(
	indexes: &mut LinkageStoreIndexes,
	reference: &ReferenceRecord,
	material: &CodeIndexMaterial,
) {
	let Some(reference_ordinal) = indexes.reference_indexes.get(&reference.id).copied() else {
		return;
	};
	if let Some(source_root) = reference_source_root(reference, material) {
		indexes
			.references_by_source_root
			.entry(source_root)
			.or_default()
			.insert(reference_ordinal);
	}
	let Some(query) = LinkageQuery::new(reference, material) else {
		return;
	};
	for key in query_keys(&query) {
		indexes
			.references_by_name
			.entry(key)
			.or_default()
			.insert(reference_ordinal);
	}
}

fn refresh_resolved_target_index(
	store: &mut LinkageStore,
	references: &ReferenceSet,
	material: &CodeIndexMaterial,
) {
	store.ensure_resolved_target_index(material);
	store.indexes.remove_resolved_references(references);
	for decision in &store.decisions {
		if store
			.indexes
			.reference_indexes
			.get(decision.reference())
			.is_some_and(|reference| references.contains(*reference))
		{
			store
				.indexes
				.add_resolved_target_indexes(decision, material, &store.symbols);
		}
	}
}

fn store_memory_metrics(store: &LinkageStore) -> LinkageMemoryMetrics {
	let mut metrics = LinkageMemoryMetrics {
		symbol_catalog_entries: store.symbols.len(),
		decisions: store.decisions.len(),
		..LinkageMemoryMetrics::default()
	};
	record_reference_sets(
		store.indexes.references_by_source_root.values(),
		&mut metrics,
	);
	record_reference_sets(store.indexes.references_by_name.values(), &mut metrics);
	if let Some(index) = &store.indexes.resolved_by_target_source {
		index.record_memory(&mut metrics);
	}
	for decision in &store.decisions {
		if let Some(targets) = decision.resolved_targets() {
			metrics.add_symbol_set(targets.len(), targets.serialized_size());
		}
	}
	metrics
}

fn record_reference_sets<'a>(
	sets: impl IntoIterator<Item = &'a ReferenceSet>,
	metrics: &mut LinkageMemoryMetrics,
) {
	for set in sets {
		metrics.add_reference_set(set.len(), set.serialized_size());
	}
}

fn decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	references: &[ReferenceRecord],
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog<'_>,
) -> Vec<ReferenceLinkageDecision> {
	let reference_indexes = reference_indexes(references);
	let mut decisions = Vec::new();
	decisions.extend(resolved_decisions_from_snapshot(
		snapshot,
		&reference_indexes,
		candidates.symbols(),
	));
	decisions.extend(external_decisions_from_snapshot(
		snapshot,
		&reference_indexes,
		material,
	));
	decisions.extend(
		snapshot
			.manifest_blocked
			.iter()
			.filter_map(|blocked| reference_indexes.get(&blocked.reference).copied())
			.map(|reference_idx| {
				ReferenceLinkageDecision::manifest_blocked(
					reference_idx.index(),
					references[reference_idx.index()].id.clone(),
				)
			}),
	);
	decisions.extend(
		snapshot
			.unresolved
			.iter()
			.filter_map(|unresolved| reference_indexes.get(&unresolved.reference).copied())
			.map(|reference_idx| {
				ReferenceLinkageDecision::unknown(
					UnknownReason::NoCandidate,
					reference_idx.index(),
					references[reference_idx.index()].id.clone(),
				)
			}),
	);
	decisions
}

fn resolved_decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
	symbols: &SymbolOrdinalCatalog,
) -> Vec<ReferenceLinkageDecision> {
	let mut targets_by_reference = BTreeMap::<ReferenceId, SymbolSet>::new();
	for edge in &snapshot.resolved {
		let Some(target) = symbols.ordinal(&edge.target) else {
			continue;
		};
		targets_by_reference
			.entry(edge.reference.clone())
			.or_default()
			.insert(target);
	}
	targets_by_reference
		.into_iter()
		.filter_map(|(reference, targets)| {
			reference_indexes.get(&reference).map(|reference_idx| {
				ReferenceLinkageDecision::resolved(
					crate::linkage::decision::ResolutionScope::Global,
					reference_idx.index(),
					reference.clone(),
					targets,
				)
			})
		})
		.collect()
}

fn external_decisions_from_snapshot(
	snapshot: &LinkageSnapshot,
	reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
	material: &CodeIndexMaterial,
) -> Vec<ReferenceLinkageDecision> {
	snapshot
		.external
		.iter()
		.filter_map(|external| {
			let reference_idx = reference_indexes.get(&external.reference)?.index();
			let target = from_uri(
				external.target_identity.as_ref(),
				&UriConfig {
					scheme: material.identity.scheme(),
				},
			)
			.ok();
			Some(match target {
				Some(target) => ReferenceLinkageDecision::external_target(
					external.origin,
					reference_idx,
					external.reference.clone(),
					target,
				),
				None => ReferenceLinkageDecision::external(
					external.origin,
					reference_idx,
					external.reference.clone(),
				),
			})
		})
		.collect()
}

#[derive(Clone)]
pub(super) struct LinkageStoreIndexes {
	pub(super) reference_indexes: FxHashMap<ReferenceId, ReferenceOrdinal>,
	pub(super) references_by_source_root: FxHashMap<usize, ReferenceSet>,
	pub(super) references_by_name: FxHashMap<Vec<u8>, ReferenceSet>,
	pub(super) resolved_by_target_source: Option<ResolvedTargetSourceIndex>,
}

impl LinkageStoreIndexes {
	fn new(references: &[ReferenceRecord], material: &CodeIndexMaterial) -> Self {
		Self::from_references(references, material)
	}

	fn from_references(references: &[ReferenceRecord], material: &CodeIndexMaterial) -> Self {
		Self {
			reference_indexes: reference_indexes(references),
			references_by_source_root: references_by_source_root(references, material),
			references_by_name: references_by_name(references, material),
			resolved_by_target_source: None,
		}
	}

	fn add_resolved_target_indexes(
		&mut self,
		decision: &ReferenceLinkageDecision,
		material: &CodeIndexMaterial,
		symbols: &SymbolOrdinalCatalog,
	) {
		let Some(index) = &mut self.resolved_by_target_source else {
			return;
		};
		add_resolved_target_decision(
			index,
			decision,
			ResolvedTargetSourceContext { material, symbols },
		);
	}

	fn rebase_reference_ordinals(&mut self, rebase: &ReferenceOrdinalRebase) {
		rebase_reference_maps(&mut self.references_by_source_root, rebase);
		rebase_reference_maps(&mut self.references_by_name, rebase);
		if let Some(index) = &mut self.resolved_by_target_source {
			index.rebase_reference_ordinals(rebase);
		}
	}

	fn rebase_symbol_ordinals(
		&mut self,
		previous: &SymbolOrdinalCatalog,
		next: &SymbolOrdinalCatalog,
		id_remaps: &FxHashMap<SymbolId, SymbolId>,
	) {
		if let Some(index) = &mut self.resolved_by_target_source {
			index.rebase_symbol_ordinals(previous, next, id_remaps);
		}
	}

	fn remove_stale_references(&mut self, stale_references: &ReferenceSet) {
		remove_references(&mut self.references_by_source_root, stale_references);
		remove_references(&mut self.references_by_name, stale_references);
		self.remove_resolved_references(stale_references);
	}

	fn remove_resolved_references(&mut self, stale_references: &ReferenceSet) {
		if let Some(index) = &mut self.resolved_by_target_source {
			index.remove_references(stale_references);
		}
	}

	fn rebuild_resolved_target_indexes(&mut self, input: ResolvedTargetSourceBuild<'_>) {
		let mut index = ResolvedTargetSourceIndex::default();
		index.collect_decisions(input);
		self.resolved_by_target_source = Some(index);
	}
}

#[derive(Clone, Copy)]
struct ResolvedTargetSourceBuild<'a> {
	decisions: &'a [ReferenceLinkageDecision],
	material: &'a CodeIndexMaterial,
	symbols: &'a SymbolOrdinalCatalog,
}

impl<'a> ResolvedTargetSourceBuild<'a> {
	fn context(self) -> ResolvedTargetSourceContext<'a> {
		ResolvedTargetSourceContext {
			material: self.material,
			symbols: self.symbols,
		}
	}
}

#[derive(Clone, Copy)]
struct ResolvedTargetSourceContext<'a> {
	material: &'a CodeIndexMaterial,
	symbols: &'a SymbolOrdinalCatalog,
}

#[derive(Clone, Default)]
pub(super) struct ResolvedTargetSourceIndex {
	references_by_source: FxHashMap<SourceId, ReferenceSet>,
	references_by_symbol: FxHashMap<SymbolOrdinal, ReferenceSet>,
}

impl ResolvedTargetSourceIndex {
	pub(super) fn get(&self, source: &SourceId) -> Option<&ReferenceSet> {
		self.references_by_source.get(source)
	}

	pub(super) fn get_symbol(&self, symbol: SymbolOrdinal) -> Option<&ReferenceSet> {
		self.references_by_symbol.get(&symbol)
	}

	fn record_memory(&self, metrics: &mut LinkageMemoryMetrics) {
		record_reference_sets(self.references_by_source.values(), metrics);
		record_reference_sets(self.references_by_symbol.values(), metrics);
	}

	fn collect_decisions(&mut self, input: ResolvedTargetSourceBuild<'_>) {
		let context = input.context();
		for decision in input.decisions {
			add_resolved_target_decision(self, decision, context);
		}
	}

	fn remove_references(&mut self, stale_references: &ReferenceSet) {
		remove_references(&mut self.references_by_source, stale_references);
		remove_references(&mut self.references_by_symbol, stale_references);
	}

	fn rebase_reference_ordinals(&mut self, rebase: &ReferenceOrdinalRebase) {
		rebase_reference_maps(&mut self.references_by_source, rebase);
		rebase_reference_maps(&mut self.references_by_symbol, rebase);
	}

	fn rebase_symbol_ordinals(
		&mut self,
		previous: &SymbolOrdinalCatalog,
		next: &SymbolOrdinalCatalog,
		id_remaps: &FxHashMap<SymbolId, SymbolId>,
	) {
		let mut rebased = FxHashMap::<SymbolOrdinal, ReferenceSet>::default();
		for (symbol, references) in &self.references_by_symbol {
			let Some(next_symbol) = previous.remap_ordinal_with_ids(*symbol, next, id_remaps)
			else {
				continue;
			};
			rebased
				.entry(next_symbol)
				.or_default()
				.union_with(references);
		}
		self.references_by_symbol = rebased;
	}
}

fn add_resolved_target_decision(
	index: &mut ResolvedTargetSourceIndex,
	decision: &ReferenceLinkageDecision,
	context: ResolvedTargetSourceContext<'_>,
) {
	let Some(targets) = decision.resolved_targets() else {
		return;
	};
	for target in targets.iter() {
		index
			.references_by_symbol
			.entry(target)
			.or_default()
			.insert(ReferenceOrdinal::from_index(decision.reference_idx()));
		let Some(symbol_id) = context.symbols.id(target) else {
			continue;
		};
		let Some(source) = context.material.symbol_source(symbol_id) else {
			continue;
		};
		index
			.references_by_source
			.entry(source)
			.or_default()
			.insert(ReferenceOrdinal::from_index(decision.reference_idx()));
	}
}

pub(super) fn reference_indexes(
	references: &[ReferenceRecord],
) -> FxHashMap<ReferenceId, ReferenceOrdinal> {
	references
		.iter()
		.enumerate()
		.map(|(idx, reference)| (reference.id.clone(), ReferenceOrdinal::from_index(idx)))
		.collect()
}

fn remove_references<K: Eq + Hash>(
	index: &mut FxHashMap<K, ReferenceSet>,
	references: &ReferenceSet,
) {
	index.retain(|_, indexed_references| {
		indexed_references.remove_all(references);
		!indexed_references.is_empty()
	});
}

fn rebase_reference_maps<K: Eq + Hash>(
	index: &mut FxHashMap<K, ReferenceSet>,
	rebase: &ReferenceOrdinalRebase,
) {
	index.retain(|_, references| {
		*references = references
			.iter()
			.filter_map(|reference| rebase.map(reference))
			.collect();
		!references.is_empty()
	});
}

fn references_by_source_root(
	references: &[ReferenceRecord],
	material: &CodeIndexMaterial,
) -> FxHashMap<usize, ReferenceSet> {
	references
		.par_iter()
		.enumerate()
		.fold(
			FxHashMap::<usize, ReferenceSet>::default,
			|mut index, item| {
				let (reference_idx, reference) = item;
				if let Some(source_root) = reference_source_root(reference, material) {
					index
						.entry(source_root)
						.or_default()
						.insert(ReferenceOrdinal::from_index(reference_idx));
				}
				index
			},
		)
		.reduce(
			FxHashMap::<usize, ReferenceSet>::default,
			merge_reference_set_maps,
		)
}

fn references_by_name(
	references: &[ReferenceRecord],
	material: &CodeIndexMaterial,
) -> FxHashMap<Vec<u8>, ReferenceSet> {
	references
		.par_iter()
		.enumerate()
		.fold(
			FxHashMap::<Vec<u8>, ReferenceSet>::default,
			|mut index, item| {
				let (reference_idx, reference) = item;
				let Some(query) = LinkageQuery::new(reference, material) else {
					return index;
				};
				for key in query_keys(&query) {
					index
						.entry(key)
						.or_default()
						.insert(ReferenceOrdinal::from_index(reference_idx));
				}
				index
			},
		)
		.reduce(
			FxHashMap::<Vec<u8>, ReferenceSet>::default,
			merge_reference_set_maps,
		)
}

fn merge_reference_set_maps<K: Eq + Hash>(
	mut left: FxHashMap<K, ReferenceSet>,
	right: FxHashMap<K, ReferenceSet>,
) -> FxHashMap<K, ReferenceSet> {
	for (key, references) in right {
		left.entry(key).or_default().union_with(&references);
	}
	left
}

fn reference_source_root(
	reference: &ReferenceRecord,
	material: &CodeIndexMaterial,
) -> Option<usize> {
	let (file_idx, _) = material.identity.reference_location(&reference.id)?;
	material.files.get(file_idx).map(|file| file.source_root)
}
