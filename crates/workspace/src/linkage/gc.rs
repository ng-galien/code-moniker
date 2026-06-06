use rustc_hash::FxHashMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use code_moniker_core::lang::build_manifest::Manifest;

use crate::linkage::candidate::{CandidateCatalog, matches_any_source};
use crate::linkage::ordinals::{ReferenceOrdinal, ReferenceSet};
use crate::linkage::query::LinkageQuery;
use crate::linkage::store::LinkageStore;
use crate::path_util::normalize_path;
use crate::snapshot::{ReferenceId, ReferenceRecord, SourceId, SymbolId};
use crate::source::CodeIndexMaterial;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkageRefreshImpact {
	changed_sources: Vec<SourceId>,
	changed_paths: Vec<PathBuf>,
	changed_references: Vec<ReferenceId>,
	removed_references: Vec<ReferenceId>,
	changed_symbols: Vec<SymbolId>,
	removed_symbols: Vec<SymbolId>,
	reference_id_remaps: Vec<(ReferenceId, ReferenceId)>,
	symbol_id_remaps: Vec<(SymbolId, SymbolId)>,
	precise_graph_diff: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkageRefreshGraphDiff {
	pub changed_references: Vec<ReferenceId>,
	pub removed_references: Vec<ReferenceId>,
	pub changed_symbols: Vec<SymbolId>,
	pub removed_symbols: Vec<SymbolId>,
	pub reference_id_remaps: Vec<(ReferenceId, ReferenceId)>,
	pub symbol_id_remaps: Vec<(SymbolId, SymbolId)>,
}

impl LinkageRefreshImpact {
	pub fn new(changed_sources: Vec<SourceId>, changed_paths: Vec<PathBuf>) -> Self {
		Self {
			changed_sources,
			changed_paths,
			changed_references: Vec::new(),
			removed_references: Vec::new(),
			changed_symbols: Vec::new(),
			removed_symbols: Vec::new(),
			reference_id_remaps: Vec::new(),
			symbol_id_remaps: Vec::new(),
			precise_graph_diff: false,
		}
	}

	pub fn with_graph_diff(
		changed_sources: Vec<SourceId>,
		changed_paths: Vec<PathBuf>,
		graph_diff: LinkageRefreshGraphDiff,
	) -> Self {
		Self {
			changed_sources,
			changed_paths,
			changed_references: graph_diff.changed_references,
			removed_references: graph_diff.removed_references,
			changed_symbols: graph_diff.changed_symbols,
			removed_symbols: graph_diff.removed_symbols,
			reference_id_remaps: graph_diff.reference_id_remaps,
			symbol_id_remaps: graph_diff.symbol_id_remaps,
			precise_graph_diff: true,
		}
	}

	pub fn is_empty(&self) -> bool {
		self.changed_sources.is_empty()
			&& self.changed_paths.is_empty()
			&& self.changed_references.is_empty()
			&& self.removed_references.is_empty()
			&& self.changed_symbols.is_empty()
			&& self.removed_symbols.is_empty()
	}

	pub(super) fn changed_references(&self) -> &[ReferenceId] {
		&self.changed_references
	}

	pub(super) fn reference_id_remaps(&self) -> &[(ReferenceId, ReferenceId)] {
		&self.reference_id_remaps
	}

	pub(super) fn symbol_id_remaps(&self) -> &[(SymbolId, SymbolId)] {
		&self.symbol_id_remaps
	}

	pub(super) fn has_precise_graph_diff(&self) -> bool {
		self.precise_graph_diff
	}
}

pub(super) struct LinkageGarbageCollector<'a> {
	store: &'a LinkageStore,
	changed_sources: BTreeSet<SourceId>,
	changed_symbols: BTreeSet<SymbolId>,
	precise_graph_diff: bool,
	changed_source_references: ReferenceSet,
	references_matching_changed_defs: ReferenceSet,
	policy_source_roots: BTreeSet<usize>,
	missing_resolved_references: ReferenceSet,
}

impl<'a> LinkageGarbageCollector<'a> {
	pub(super) fn new(
		store: &'a LinkageStore,
		references: &'a [ReferenceRecord],
		material: &'a CodeIndexMaterial,
		candidates: &'a CandidateCatalog<'a>,
		reference_indexes: &'a FxHashMap<ReferenceId, ReferenceOrdinal>,
		impact: &LinkageRefreshImpact,
	) -> Self {
		build_garbage_collector(
			store,
			references,
			material,
			candidates,
			reference_indexes,
			impact,
		)
	}

	pub(super) fn collect(&self) -> ReferenceSet {
		let mut stale = ReferenceSet::new();
		self.mark_changed_source_references(&mut stale);
		self.mark_policy_references(&mut stale);
		self.mark_references_matching_changed_defs(&mut stale);
		self.mark_resolved_target_references(&mut stale);
		stale.union_with(&self.missing_resolved_references);
		stale
	}

	fn mark_changed_source_references(&self, stale: &mut ReferenceSet) {
		stale.union_with(&self.changed_source_references);
	}

	fn mark_policy_references(&self, stale: &mut ReferenceSet) {
		for root in &self.policy_source_roots {
			if let Some(references) = self.store.indexes.references_by_source_root.get(root) {
				stale.union_with(references);
			}
		}
	}

	fn mark_references_matching_changed_defs(&self, stale: &mut ReferenceSet) {
		stale.union_with(&self.references_matching_changed_defs);
	}

	fn mark_resolved_target_references(&self, stale: &mut ReferenceSet) {
		if self.precise_graph_diff && self.changed_symbols.is_empty() {
			return;
		}
		let Some(resolved_by_target_source) = &self.store.indexes.resolved_by_target_source else {
			return;
		};
		for source in &self.changed_sources {
			if let Some(references) = resolved_by_target_source.get(source) {
				stale.union_with(references);
			}
		}
	}
}

fn build_garbage_collector<'a>(
	store: &'a LinkageStore,
	references: &'a [ReferenceRecord],
	material: &'a CodeIndexMaterial,
	candidates: &'a CandidateCatalog<'a>,
	reference_indexes: &'a FxHashMap<ReferenceId, ReferenceOrdinal>,
	impact: &LinkageRefreshImpact,
) -> LinkageGarbageCollector<'a> {
	let changed_sources = changed_sources(impact);
	let changed_symbols = changed_symbols(impact);
	let changed_source_files = changed_source_files(material, &changed_sources);
	LinkageGarbageCollector {
		store,
		changed_source_references: changed_source_references(
			references,
			reference_indexes,
			impact,
			&changed_sources,
		),
		references_matching_changed_defs: references_matching_changed_defs(
			ChangedDefReferenceInput {
				store,
				references,
				material,
				candidates,
				impact,
				changed_symbols: &changed_symbols,
				changed_source_files: &changed_source_files,
			},
		),
		policy_source_roots: policy_source_roots(material, &impact.changed_paths),
		missing_resolved_references: reference_set_for_ids(
			store.missing_resolved_references(material),
			reference_indexes,
		),
		changed_sources,
		changed_symbols,
		precise_graph_diff: impact.has_precise_graph_diff(),
	}
}

pub(super) fn changed_file_indexes(
	material: &CodeIndexMaterial,
	impact: &LinkageRefreshImpact,
) -> BTreeSet<usize> {
	changed_source_files(material, &changed_sources(impact))
}

fn changed_sources(impact: &LinkageRefreshImpact) -> BTreeSet<SourceId> {
	impact.changed_sources.iter().cloned().collect()
}

fn changed_symbols(impact: &LinkageRefreshImpact) -> BTreeSet<SymbolId> {
	impact
		.changed_symbols
		.iter()
		.chain(impact.removed_symbols.iter())
		.cloned()
		.collect()
}

fn changed_source_references(
	references: &[ReferenceRecord],
	reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
	impact: &LinkageRefreshImpact,
	changed_sources: &BTreeSet<SourceId>,
) -> ReferenceSet {
	if impact.has_precise_graph_diff() {
		return reference_set_for_ids(impact.changed_references().to_vec(), reference_indexes);
	}
	references
		.iter()
		.enumerate()
		.filter(|(_, reference)| changed_sources.contains(&reference.source))
		.map(|(reference_idx, _)| ReferenceOrdinal::from_index(reference_idx))
		.collect()
}

fn changed_source_files(
	material: &CodeIndexMaterial,
	changed_sources: &BTreeSet<SourceId>,
) -> BTreeSet<usize> {
	material
		.files
		.iter()
		.enumerate()
		.filter(|(_, file)| changed_sources.contains(&file.source_id))
		.map(|(file_idx, _)| file_idx)
		.collect()
}

struct ChangedDefReferenceInput<'a> {
	store: &'a LinkageStore,
	references: &'a [ReferenceRecord],
	material: &'a CodeIndexMaterial,
	candidates: &'a CandidateCatalog<'a>,
	impact: &'a LinkageRefreshImpact,
	changed_symbols: &'a BTreeSet<SymbolId>,
	changed_source_files: &'a BTreeSet<usize>,
}

fn references_matching_changed_defs(input: ChangedDefReferenceInput<'_>) -> ReferenceSet {
	if input.impact.has_precise_graph_diff() && input.changed_symbols.is_empty() {
		return ReferenceSet::new();
	}
	if input.changed_source_files.is_empty() {
		return ReferenceSet::new();
	}
	let mut seen = ReferenceSet::new();
	let mut stale = ReferenceSet::new();
	for source_file in input.changed_source_files {
		let Some(keys) = input
			.candidates
			.indexes()
			.source_candidate_keys(*source_file)
		else {
			continue;
		};
		for key in keys {
			let Some(ids) = input.store.indexes.references_by_name.get(key) else {
				continue;
			};
			for reference_ordinal in ids.iter() {
				if !seen.insert(reference_ordinal) {
					continue;
				}
				let Some(record) = input.references.get(reference_ordinal.index()) else {
					continue;
				};
				let Some(query) = LinkageQuery::new(record, input.material) else {
					continue;
				};
				if matches_any_source(input.candidates, &query, input.changed_source_files) {
					stale.insert(reference_ordinal);
				}
			}
		}
	}
	stale
}

fn reference_set_for_ids(
	references: Vec<ReferenceId>,
	reference_indexes: &FxHashMap<ReferenceId, ReferenceOrdinal>,
) -> ReferenceSet {
	references
		.into_iter()
		.filter_map(|reference| reference_indexes.get(&reference).copied())
		.collect()
}

fn policy_source_roots(material: &CodeIndexMaterial, paths: &[PathBuf]) -> BTreeSet<usize> {
	paths
		.iter()
		.filter(|path| Manifest::for_filename(path).is_some())
		.filter_map(|path| source_root_for_path(material, path))
		.collect()
}

fn source_root_for_path(material: &CodeIndexMaterial, path: &Path) -> Option<usize> {
	let path = normalize_path(path);
	material
		.source_catalog
		.sources
		.roots
		.iter()
		.enumerate()
		.filter_map(|(root_idx, root)| {
			let root_path = normalize_path(&root.path);
			path.starts_with(&root_path)
				.then_some((root_idx, root_path.components().count()))
		})
		.max_by_key(|(_, depth)| *depth)
		.map(|(root_idx, _)| root_idx)
}
