use std::collections::HashSet;
use std::hash::Hash;

use rayon::prelude::*;
use rustc_hash::FxHashMap;

use super::{LinkageMemoryMetrics, ReferenceLinkageDecision};
use crate::linkage::catalog::{
	LinkageQuery, ReferenceOrdinal, ReferenceSet, SymbolOrdinal, SymbolOrdinalCatalog, query_keys,
};
use crate::snapshot::{RecordTable, ReferenceId, ReferenceRecord, SourceId, SymbolId};
use crate::source::CodeIndexMaterial;

#[derive(Clone)]
pub(in crate::linkage) struct LinkageStoreIndexes {
	pub(in crate::linkage) reference_indexes: FxHashMap<ReferenceId, ReferenceOrdinal>,
	pub(in crate::linkage) references_by_source_root: FxHashMap<usize, ReferenceSet>,
	pub(in crate::linkage) references_by_name: FxHashMap<Vec<u8>, ReferenceSet>,
	pub(in crate::linkage) references_by_source_symbol: FxHashMap<SymbolId, ReferenceSet>,
	pub(in crate::linkage) references_by_call_name: FxHashMap<Vec<u8>, ReferenceSet>,
	pub(in crate::linkage) resolved_by_target_source: Option<ResolvedTargetSourceIndex>,
}

impl LinkageStoreIndexes {
	pub(super) fn new(
		references: &RecordTable<ReferenceRecord>,
		material: &CodeIndexMaterial,
	) -> Self {
		Self {
			reference_indexes: reference_indexes(references),
			references_by_source_root: parallel_reference_index(references, |reference| {
				reference_source_root(reference, material)
			}),
			references_by_name: parallel_reference_index(references, |reference| {
				LinkageQuery::new(reference, material)
					.map(|query| query_keys(&query))
					.unwrap_or_default()
			}),
			references_by_source_symbol: references_by_source_symbol(references),
			references_by_call_name: references_by_call_name(references),
			resolved_by_target_source: None,
		}
	}

	pub(super) fn add_reference(
		&mut self,
		reference: &ReferenceRecord,
		material: &CodeIndexMaterial,
	) {
		let Some(reference_ordinal) = self.reference_indexes.get(&reference.id).copied() else {
			return;
		};
		if let Some(source_root) = reference_source_root(reference, material) {
			self.references_by_source_root
				.entry(source_root)
				.or_default()
				.insert(reference_ordinal);
		}
		self.references_by_source_symbol
			.entry(reference.source_symbol)
			.or_default()
			.insert(reference_ordinal);
		if let Some(call_name) = reference.call_name.as_deref() {
			self.references_by_call_name
				.entry(call_name.as_bytes().to_vec())
				.or_default()
				.insert(reference_ordinal);
		}
		let Some(query) = LinkageQuery::new(reference, material) else {
			return;
		};
		for key in query_keys(&query) {
			self.references_by_name
				.entry(key)
				.or_default()
				.insert(reference_ordinal);
		}
	}

	pub(super) fn add_resolved_target(
		&mut self,
		decision: &ReferenceLinkageDecision,
		material: &CodeIndexMaterial,
		symbols: &SymbolOrdinalCatalog,
	) {
		let Some(index) = &mut self.resolved_by_target_source else {
			return;
		};
		index.add_decision(decision, ResolvedTargetContext { material, symbols });
	}

	pub(super) fn rebase_reference_ordinals(&mut self, rebase: &ReferenceOrdinalRebase) {
		rebase_reference_maps(&mut self.references_by_source_root, rebase);
		rebase_reference_maps(&mut self.references_by_name, rebase);
		rebase_reference_maps(&mut self.references_by_source_symbol, rebase);
		rebase_reference_maps(&mut self.references_by_call_name, rebase);
		if let Some(index) = &mut self.resolved_by_target_source {
			index.rebase_reference_ordinals(rebase);
		}
	}

	pub(super) fn remove_stale_references(&mut self, stale_references: &ReferenceSet) {
		remove_references(&mut self.references_by_source_root, stale_references);
		remove_references(&mut self.references_by_name, stale_references);
		remove_references(&mut self.references_by_source_symbol, stale_references);
		remove_references(&mut self.references_by_call_name, stale_references);
		self.remove_resolved_references(stale_references);
	}

	pub(super) fn remove_resolved_references(&mut self, stale_references: &ReferenceSet) {
		if let Some(index) = &mut self.resolved_by_target_source {
			index.remove_references(stale_references);
		}
	}

	pub(super) fn rebuild_resolved_targets(
		&mut self,
		decisions: &[ReferenceLinkageDecision],
		material: &CodeIndexMaterial,
		symbols: &SymbolOrdinalCatalog,
	) {
		self.resolved_by_target_source = Some(ResolvedTargetSourceIndex::from_decisions(
			decisions, material, symbols,
		));
	}

	pub(super) fn record_memory(&self, metrics: &mut LinkageMemoryMetrics) {
		record_reference_sets(self.references_by_source_root.values(), metrics);
		record_reference_sets(self.references_by_name.values(), metrics);
		record_reference_sets(self.references_by_source_symbol.values(), metrics);
		record_reference_sets(self.references_by_call_name.values(), metrics);
		if let Some(index) = &self.resolved_by_target_source {
			record_reference_sets(index.references_by_source.values(), metrics);
			record_reference_sets(index.references_by_symbol.values(), metrics);
		}
	}
}

#[derive(Clone, Default)]
pub(in crate::linkage) struct ResolvedTargetSourceIndex {
	references_by_source: FxHashMap<SourceId, ReferenceSet>,
	references_by_symbol: FxHashMap<SymbolOrdinal, ReferenceSet>,
}

impl ResolvedTargetSourceIndex {
	pub(in crate::linkage) fn get(&self, source: &SourceId) -> Option<&ReferenceSet> {
		self.references_by_source.get(source)
	}

	pub(in crate::linkage) fn get_symbol(&self, symbol: SymbolOrdinal) -> Option<&ReferenceSet> {
		self.references_by_symbol.get(&symbol)
	}

	fn from_decisions(
		decisions: &[ReferenceLinkageDecision],
		material: &CodeIndexMaterial,
		symbols: &SymbolOrdinalCatalog,
	) -> Self {
		let mut index = Self::default();
		let context = ResolvedTargetContext { material, symbols };
		for decision in decisions {
			index.add_decision(decision, context);
		}
		index
	}

	fn add_decision(
		&mut self,
		decision: &ReferenceLinkageDecision,
		context: ResolvedTargetContext<'_>,
	) {
		let Some(targets) = decision.linkage_targets() else {
			return;
		};
		for target in targets.iter() {
			self.references_by_symbol
				.entry(target)
				.or_default()
				.insert(ReferenceOrdinal::from_index(decision.reference_idx()));
			let Some(symbol_id) = context.symbols.id(target) else {
				continue;
			};
			let Some(source) = context.material.symbol_source(symbol_id) else {
				continue;
			};
			self.references_by_source
				.entry(source)
				.or_default()
				.insert(ReferenceOrdinal::from_index(decision.reference_idx()));
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
}

#[derive(Clone, Copy)]
struct ResolvedTargetContext<'a> {
	material: &'a CodeIndexMaterial,
	symbols: &'a SymbolOrdinalCatalog,
}

pub(super) struct ReferenceOrdinalRebase {
	next_by_old: Vec<Option<ReferenceOrdinal>>,
}

impl ReferenceOrdinalRebase {
	pub(super) fn new(
		previous: &FxHashMap<ReferenceId, ReferenceOrdinal>,
		next: &FxHashMap<ReferenceId, ReferenceOrdinal>,
		reference_id_remaps: &[(ReferenceId, ReferenceId)],
		removed_references: &HashSet<&ReferenceId>,
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
			if removed_references.contains(reference) {
				continue;
			}
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

pub(in crate::linkage) fn reference_indexes(
	references: &RecordTable<ReferenceRecord>,
) -> FxHashMap<ReferenceId, ReferenceOrdinal> {
	references
		.iter()
		.enumerate()
		.map(|(idx, reference)| (reference.id, ReferenceOrdinal::from_index(idx)))
		.collect()
}

fn parallel_reference_index<K, I>(
	references: &RecordTable<ReferenceRecord>,
	keys: impl Fn(&ReferenceRecord) -> I + Sync,
) -> FxHashMap<K, ReferenceSet>
where
	K: Eq + Hash + Send,
	I: IntoIterator<Item = K>,
{
	(0..references.len())
		.into_par_iter()
		.fold(
			FxHashMap::<K, ReferenceSet>::default,
			|mut index, reference_idx| {
				for key in keys(&references[reference_idx]) {
					index
						.entry(key)
						.or_default()
						.insert(ReferenceOrdinal::from_index(reference_idx));
				}
				index
			},
		)
		.reduce(FxHashMap::default, merge_reference_set_maps)
}

fn references_by_source_symbol(
	references: &RecordTable<ReferenceRecord>,
) -> FxHashMap<SymbolId, ReferenceSet> {
	let mut index = FxHashMap::default();
	for (reference_idx, reference) in references.iter().enumerate() {
		index
			.entry(reference.source_symbol)
			.or_insert_with(ReferenceSet::new)
			.insert(ReferenceOrdinal::from_index(reference_idx));
	}
	index
}

fn references_by_call_name(
	references: &RecordTable<ReferenceRecord>,
) -> FxHashMap<Vec<u8>, ReferenceSet> {
	let mut index = FxHashMap::default();
	for (reference_idx, reference) in references.iter().enumerate() {
		let Some(call_name) = reference.call_name.as_deref() else {
			continue;
		};
		index
			.entry(call_name.as_bytes().to_vec())
			.or_insert_with(ReferenceSet::new)
			.insert(ReferenceOrdinal::from_index(reference_idx));
	}
	index
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

fn record_reference_sets<'a>(
	sets: impl IntoIterator<Item = &'a ReferenceSet>,
	metrics: &mut LinkageMemoryMetrics,
) {
	for set in sets {
		metrics.add_reference_set(set.len(), set.serialized_size());
	}
}
