use crate::linkage::binding::LinkageMemoryMetrics;
use crate::linkage::binding::ReferenceLinkageDecision;
use crate::linkage::catalog::{
	CandidateCatalog, ReferenceOrdinal, ReferenceSet, SymbolOrdinal, SymbolOrdinalCatalog,
};
use crate::snapshot::{
	LinkageSnapshot, RecordTable, ReferenceId, ReferenceRecord, ResourceGeneration,
};
use crate::source::CodeIndexMaterial;
use rustc_hash::FxHashMap;

use super::indexes::{LinkageStoreIndexes, ReferenceOrdinalRebase};

#[derive(Clone)]
pub(in crate::linkage) struct LinkageStore {
	generation: ResourceGeneration,
	index_generation: ResourceGeneration,
	decisions: Vec<ReferenceLinkageDecision>,
	pub(in crate::linkage) indexes: LinkageStoreIndexes,
}

pub(in crate::linkage) struct LinkageStoreRefresh<'a> {
	pub(in crate::linkage) generation: ResourceGeneration,
	pub(in crate::linkage) index_generation: ResourceGeneration,
	pub(in crate::linkage) stale_references: &'a ReferenceSet,
	pub(in crate::linkage) changed_decisions: Vec<ReferenceLinkageDecision>,
	pub(in crate::linkage) references: &'a RecordTable<ReferenceRecord>,
	pub(in crate::linkage) material: &'a CodeIndexMaterial,
}

impl LinkageStore {
	pub(in crate::linkage) fn new(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		decisions: Vec<ReferenceLinkageDecision>,
		references: &RecordTable<ReferenceRecord>,
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog,
	) -> Self {
		let mut indexes = LinkageStoreIndexes::new(references, material);
		indexes.rebuild_resolved_targets(&decisions, material, candidates.symbols());
		Self {
			generation,
			index_generation,
			decisions,
			indexes,
		}
	}

	pub(in crate::linkage) fn from_snapshot(
		snapshot: &LinkageSnapshot,
		references: &RecordTable<ReferenceRecord>,
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog,
	) -> Self {
		Self::new(
			snapshot.generation,
			snapshot.index_generation,
			super::restore::decisions_from_snapshot(snapshot, references, material, candidates),
			references,
			material,
			candidates,
		)
	}

	pub(in crate::linkage) fn project_snapshot(
		&self,
		references: &RecordTable<ReferenceRecord>,
		material: &CodeIndexMaterial,
		symbols: std::sync::Arc<SymbolOrdinalCatalog>,
	) -> LinkageSnapshot {
		let mut snapshot = crate::linkage::binding::project_decisions(
			&self.decisions,
			references,
			&material.identity,
			&symbols,
		)
		.into_snapshot(self.generation, self.index_generation);
		snapshot.read_index = crate::snapshot::LinkageReadIndexHandle::from_snapshot_with_catalog(
			&snapshot,
			references,
			symbols,
			material
				.files
				.iter()
				.map(|file| file.source_root as u32)
				.collect(),
		);
		snapshot
	}

	pub(in crate::linkage) fn advance_index_generation(
		&mut self,
		index_generation: ResourceGeneration,
	) {
		self.index_generation = index_generation;
	}

	pub(in crate::linkage) fn apply_refresh(&mut self, refresh: LinkageStoreRefresh<'_>) {
		apply_store_refresh(self, refresh);
	}

	pub(in crate::linkage) fn rebase_reference_ordinals(
		&mut self,
		next_reference_indexes: FxHashMap<ReferenceId, ReferenceOrdinal>,
		reference_id_remaps: &[(ReferenceId, ReferenceId)],
		removed_references: &[ReferenceId],
	) {
		rebase_store_reference_ordinals(
			self,
			next_reference_indexes,
			reference_id_remaps,
			removed_references,
		);
	}

	pub(in crate::linkage) fn missing_resolved_references(
		&self,
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog,
	) -> Vec<ReferenceId> {
		missing_resolved_references(self, material, candidates)
	}

	pub(in crate::linkage) fn decisions_mut(&mut self) -> &mut [ReferenceLinkageDecision] {
		&mut self.decisions
	}

	pub(in crate::linkage) fn memory_metrics(
		&self,
		symbols: &SymbolOrdinalCatalog,
	) -> LinkageMemoryMetrics {
		store_memory_metrics(self, symbols)
	}

	pub(in crate::linkage) fn refresh_resolved_target_index(
		&mut self,
		references: &ReferenceSet,
		material: &CodeIndexMaterial,
		symbols: &SymbolOrdinalCatalog,
	) {
		refresh_resolved_target_index(self, references, material, symbols);
	}

	pub(in crate::linkage) fn ensure_resolved_target_index(
		&mut self,
		material: &CodeIndexMaterial,
		symbols: &SymbolOrdinalCatalog,
	) {
		if self.indexes.resolved_by_target_source.is_some() {
			return;
		}
		self.indexes
			.rebuild_resolved_targets(&self.decisions, material, symbols);
	}
}

fn apply_store_refresh(store: &mut LinkageStore, refresh: LinkageStoreRefresh<'_>) {
	let LinkageStoreRefresh {
		generation,
		index_generation,
		stale_references,
		changed_decisions,
		references,
		material,
	} = refresh;
	store.generation = generation;
	store.index_generation = index_generation;
	store.indexes.remove_stale_references(stale_references);
	remove_stale_decisions(store, stale_references);
	add_changed_decisions(store, changed_decisions, references, material);
}

fn missing_resolved_references(
	store: &LinkageStore,
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog,
) -> Vec<ReferenceId> {
	store
		.decisions
		.iter()
		.filter(|decision| {
			!store
				.indexes
				.reference_indexes
				.contains_key(decision.reference())
				|| decision.linkage_targets().is_some_and(|targets| {
					targets.iter().any(|target| {
						resolved_target_missing_or_retargeted(material, candidates, target)
					})
				})
		})
		.map(|decision| *decision.reference())
		.collect()
}

fn resolved_target_missing_or_retargeted(
	material: &CodeIndexMaterial,
	candidates: &CandidateCatalog,
	target: SymbolOrdinal,
) -> bool {
	let Some(id) = candidates.symbols().id(target) else {
		return true;
	};
	let Some(candidate) = candidates.candidate(target) else {
		return true;
	};
	let Some(current_moniker) = material.symbol_moniker(id) else {
		return true;
	};
	current_moniker != candidate.moniker
}

pub(in crate::linkage) fn insert_reference_ordinals(
	store: &mut LinkageStore,
	changed_references: &[ReferenceId],
	references: &RecordTable<ReferenceRecord>,
	material: &CodeIndexMaterial,
) {
	if changed_references.is_empty() {
		return;
	}
	let mut prefix = Vec::with_capacity(material.files.len() + 1);
	let mut total = 0usize;
	prefix.push(0usize);
	for slot in 0..material.files.len() {
		total += references.file_records(slot).len();
		prefix.push(total);
	}
	for reference in changed_references {
		let Some((slot, ref_idx)) = material.identity.reference_location(reference) else {
			continue;
		};
		let Some(base) = prefix.get(slot) else {
			continue;
		};
		store
			.indexes
			.reference_indexes
			.insert(*reference, ReferenceOrdinal::from_index(base + ref_idx));
	}
}

fn rebase_store_reference_ordinals(
	store: &mut LinkageStore,
	next_reference_indexes: FxHashMap<ReferenceId, ReferenceOrdinal>,
	reference_id_remaps: &[(ReferenceId, ReferenceId)],
	removed_references: &[ReferenceId],
) {
	let removed_references = removed_references
		.iter()
		.collect::<std::collections::HashSet<_>>();
	let rebase = ReferenceOrdinalRebase::new(
		&store.indexes.reference_indexes,
		&next_reference_indexes,
		reference_id_remaps,
		&removed_references,
	);
	store.indexes.rebase_reference_ordinals(&rebase);
	rebase_decision_references(
		store,
		&next_reference_indexes,
		reference_id_remaps,
		&removed_references,
	);
	store.indexes.reference_indexes = next_reference_indexes;
}

fn rebase_decision_references(
	store: &mut LinkageStore,
	next_reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
	reference_id_remaps: &[(ReferenceId, ReferenceId)],
	removed_references: &std::collections::HashSet<&ReferenceId>,
) {
	let reference_id_remaps = reference_id_remaps
		.iter()
		.cloned()
		.collect::<FxHashMap<ReferenceId, ReferenceId>>();
	store.decisions.retain_mut(|decision| {
		let current_reference = *decision.reference();
		if removed_references.contains(&current_reference) {
			return false;
		}
		let next_reference = reference_id_remaps
			.get(&current_reference)
			.unwrap_or(&current_reference);
		let Some(next_reference_idx) = next_reference_indexes.get(next_reference) else {
			return false;
		};
		if next_reference == &current_reference {
			decision.set_reference_idx(next_reference_idx.index());
		} else {
			decision.set_reference(*next_reference, next_reference_idx.index());
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

fn add_changed_decisions(
	store: &mut LinkageStore,
	decisions: Vec<ReferenceLinkageDecision>,
	references: &RecordTable<ReferenceRecord>,
	material: &CodeIndexMaterial,
) {
	for decision in decisions {
		let Some(reference) = references.get(decision.reference_idx()) else {
			continue;
		};
		store.indexes.add_reference(reference, material);
		store.decisions.push(decision);
	}
}

fn refresh_resolved_target_index(
	store: &mut LinkageStore,
	references: &ReferenceSet,
	material: &CodeIndexMaterial,
	symbols: &SymbolOrdinalCatalog,
) {
	store.ensure_resolved_target_index(material, symbols);
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
				.add_resolved_target(decision, material, symbols);
		}
	}
}

fn store_memory_metrics(
	store: &LinkageStore,
	symbols: &SymbolOrdinalCatalog,
) -> LinkageMemoryMetrics {
	let mut metrics = LinkageMemoryMetrics {
		symbol_catalog_entries: symbols.len(),
		decisions: store.decisions.len(),
		..LinkageMemoryMetrics::default()
	};
	store.indexes.record_memory(&mut metrics);
	for decision in &store.decisions {
		if let Some(targets) = decision.linkage_targets() {
			metrics.add_symbol_set(targets.len(), targets.serialized_size());
		}
	}
	metrics
}
