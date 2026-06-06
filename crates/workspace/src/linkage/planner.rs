use rustc_hash::FxHashMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use code_moniker_core::lang::build_manifest::Manifest;

use crate::linkage::candidate::{CandidateCatalog, matches_any_source, matches_any_symbol};
use crate::linkage::delta::{
	LinkageRefreshImpact, changed_reference_ids, changed_symbol_ids, primary_changed_symbol_ids,
	reference_id_remaps, retargeted_symbol_identities,
};
use crate::linkage::ordinals::{ReferenceOrdinal, ReferenceSet, SymbolSet};
use crate::linkage::query::LinkageQuery;
use crate::linkage::store::LinkageStore;
use crate::path_util::normalize_path;
use crate::snapshot::{ReferenceId, ReferenceRecord, SourceId};
use crate::source::CodeIndexMaterial;

pub(super) struct LinkagePlanContext<'a> {
	pub(super) store: &'a LinkageStore,
	pub(super) references: &'a [ReferenceRecord],
	pub(super) material: &'a CodeIndexMaterial,
	pub(super) candidates: &'a CandidateCatalog<'a>,
	pub(super) reference_indexes: &'a FxHashMap<ReferenceId, ReferenceOrdinal>,
	pub(super) impact: &'a LinkageRefreshImpact,
}

pub(super) struct LinkagePlanExecution {
	stale_references: ReferenceSet,
	target_index_references: ReferenceSet,
	changed_files: BTreeSet<usize>,
}

struct InvalidationPostings {
	changed_references: ReferenceSet,
	manifest_policy: ReferenceSet,
	changed_definitions: ReferenceSet,
	retargeted_targets: ReferenceSet,
	missing_targets: ReferenceSet,
}

pub(super) fn execute_linkage_plan(context: LinkagePlanContext<'_>) -> LinkagePlanExecution {
	let changed_sources = changed_sources(context.impact);
	let changed_files = changed_source_files(context.material, &changed_sources);
	let postings = plan_invalidation_postings(&context, &changed_sources, &changed_files);
	let stale_references = union_invalidation_postings(postings);
	let target_index_references = target_index_refresh_set(&context, &stale_references);
	LinkagePlanExecution {
		stale_references,
		target_index_references,
		changed_files,
	}
}

impl LinkagePlanExecution {
	pub(super) fn stale_references(&self) -> &ReferenceSet {
		&self.stale_references
	}

	pub(super) fn target_index_references(&self) -> &ReferenceSet {
		&self.target_index_references
	}

	pub(super) fn changed_files(&self) -> &BTreeSet<usize> {
		&self.changed_files
	}
}

fn plan_invalidation_postings(
	context: &LinkagePlanContext<'_>,
	changed_sources: &BTreeSet<SourceId>,
	changed_files: &BTreeSet<usize>,
) -> InvalidationPostings {
	InvalidationPostings {
		changed_references: changed_reference_postings(context, changed_sources),
		manifest_policy: manifest_policy_postings(context),
		changed_definitions: changed_definition_postings(context, changed_files),
		retargeted_targets: retargeted_target_postings(context, changed_sources),
		missing_targets: missing_target_postings(context),
	}
}

fn union_invalidation_postings(postings: InvalidationPostings) -> ReferenceSet {
	let mut stale = postings.changed_references;
	stale.union_with(&postings.manifest_policy);
	stale.union_with(&postings.changed_definitions);
	stale.union_with(&postings.retargeted_targets);
	stale.union_with(&postings.missing_targets);
	stale
}

fn changed_reference_postings(
	context: &LinkagePlanContext<'_>,
	changed_sources: &BTreeSet<SourceId>,
) -> ReferenceSet {
	if context.impact.has_precise_graph_diff() {
		return reference_set_for_ids(
			changed_reference_ids(context.impact).to_vec(),
			context.reference_indexes,
		);
	}
	source_reference_postings(context.references, changed_sources)
}

fn manifest_policy_postings(context: &LinkagePlanContext<'_>) -> ReferenceSet {
	let roots = policy_source_roots(context.material, context.impact.changed_paths());
	root_reference_postings(context.store, &roots)
}

fn changed_definition_postings(
	context: &LinkagePlanContext<'_>,
	changed_files: &BTreeSet<usize>,
) -> ReferenceSet {
	if context.impact.has_precise_graph_diff() {
		let symbols = changed_candidate_symbols(context);
		return symbol_name_postings(context, &symbols);
	}
	source_candidate_name_postings(context, changed_files)
}

fn retargeted_target_postings(
	context: &LinkagePlanContext<'_>,
	changed_sources: &BTreeSet<SourceId>,
) -> ReferenceSet {
	if context.impact.has_precise_graph_diff() {
		let identities = retargeted_symbol_identities(context.impact)
			.iter()
			.cloned()
			.collect::<BTreeSet<_>>();
		return symbol_target_postings(context.store, &identities);
	}
	target_source_postings(context.store, changed_sources)
}

fn missing_target_postings(context: &LinkagePlanContext<'_>) -> ReferenceSet {
	reference_set_for_ids(
		context
			.store
			.missing_resolved_references(context.material, context.candidates),
		context.reference_indexes,
	)
}

fn changed_sources(impact: &LinkageRefreshImpact) -> BTreeSet<SourceId> {
	impact.changed_sources().iter().cloned().collect()
}

fn changed_candidate_symbols(context: &LinkagePlanContext<'_>) -> SymbolSet {
	let symbols = primary_changed_symbol_ids(context.impact)
		.iter()
		.filter_map(|symbol| {
			context
				.candidates
				.candidate_for_symbol_id(symbol)
				.map(|(symbol, _)| symbol)
		})
		.collect::<SymbolSet>();
	if !symbols.is_empty() || !context.impact.has_precise_graph_diff() {
		return symbols;
	}
	changed_symbol_ids(context.impact)
		.iter()
		.filter_map(|symbol| {
			context
				.candidates
				.candidate_for_symbol_id(symbol)
				.map(|(symbol, _)| symbol)
		})
		.collect()
}

fn source_reference_postings(
	references: &[ReferenceRecord],
	sources: &BTreeSet<SourceId>,
) -> ReferenceSet {
	references
		.iter()
		.enumerate()
		.filter(|(_, reference)| sources.contains(&reference.source))
		.map(|(reference_idx, _)| ReferenceOrdinal::from_index(reference_idx))
		.collect()
}

fn root_reference_postings(store: &LinkageStore, roots: &BTreeSet<usize>) -> ReferenceSet {
	let mut references = ReferenceSet::new();
	for root in roots {
		if let Some(root_references) = store.indexes.references_by_source_root.get(root) {
			references.union_with(root_references);
		}
	}
	references
}

fn symbol_name_postings(context: &LinkagePlanContext<'_>, symbols: &SymbolSet) -> ReferenceSet {
	let mut seen = ReferenceSet::new();
	let mut stale = ReferenceSet::new();
	for key in changed_candidate_keys(context.candidates, symbols) {
		let Some(ids) = context.store.indexes.references_by_name.get(&key) else {
			continue;
		};
		collect_matching_symbol_postings(context, ids, symbols, &mut seen, &mut stale);
	}
	stale
}

fn source_candidate_name_postings(
	context: &LinkagePlanContext<'_>,
	files: &BTreeSet<usize>,
) -> ReferenceSet {
	let mut seen = ReferenceSet::new();
	let mut stale = ReferenceSet::new();
	for source_file in files {
		let Some(keys) = context
			.candidates
			.indexes()
			.source_candidate_keys(*source_file)
		else {
			continue;
		};
		for key in keys {
			let Some(ids) = context.store.indexes.references_by_name.get(key) else {
				continue;
			};
			collect_matching_source_postings(context, ids, files, &mut seen, &mut stale);
		}
	}
	stale
}

fn collect_matching_symbol_postings(
	context: &LinkagePlanContext<'_>,
	ids: &ReferenceSet,
	symbols: &SymbolSet,
	seen: &mut ReferenceSet,
	stale: &mut ReferenceSet,
) {
	for reference_ordinal in ids.iter() {
		if !seen.insert(reference_ordinal) {
			continue;
		}
		let Some(record) = context.references.get(reference_ordinal.index()) else {
			continue;
		};
		let Some(query) = LinkageQuery::new(record, context.material) else {
			continue;
		};
		if matches_any_symbol(context.candidates, &query, symbols) {
			stale.insert(reference_ordinal);
		}
	}
}

fn collect_matching_source_postings(
	context: &LinkagePlanContext<'_>,
	ids: &ReferenceSet,
	files: &BTreeSet<usize>,
	seen: &mut ReferenceSet,
	stale: &mut ReferenceSet,
) {
	for reference_ordinal in ids.iter() {
		if !seen.insert(reference_ordinal) {
			continue;
		}
		let Some(record) = context.references.get(reference_ordinal.index()) else {
			continue;
		};
		let Some(query) = LinkageQuery::new(record, context.material) else {
			continue;
		};
		if matches_any_source(context.candidates, &query, files) {
			stale.insert(reference_ordinal);
		}
	}
}

fn symbol_target_postings(store: &LinkageStore, identities: &BTreeSet<String>) -> ReferenceSet {
	let mut references = ReferenceSet::new();
	let Some(index) = &store.indexes.resolved_by_target_source else {
		return references;
	};
	for identity in identities {
		let Some(ordinal) = store.symbols.ordinal_by_identity(identity) else {
			continue;
		};
		if let Some(symbol_references) = index.get_symbol(ordinal) {
			references.union_with(symbol_references);
		}
	}
	references
}

fn target_source_postings(store: &LinkageStore, sources: &BTreeSet<SourceId>) -> ReferenceSet {
	let mut references = ReferenceSet::new();
	let Some(index) = &store.indexes.resolved_by_target_source else {
		return references;
	};
	for source in sources {
		if let Some(source_references) = index.get(source) {
			references.union_with(source_references);
		}
	}
	references
}

fn changed_candidate_keys(
	candidates: &CandidateCatalog<'_>,
	changed_symbols: &SymbolSet,
) -> Vec<Vec<u8>> {
	let mut keys = Vec::new();
	for symbol in changed_symbols.iter() {
		let Some(symbol_keys) = candidates.query_keys_for_symbol(symbol) else {
			continue;
		};
		for key in symbol_keys {
			push_unique_key(&mut keys, key);
		}
	}
	keys
}

fn push_unique_key(keys: &mut Vec<Vec<u8>>, key: Vec<u8>) {
	if !keys.iter().any(|existing| existing == &key) {
		keys.push(key);
	}
}

fn target_index_refresh_set(
	context: &LinkagePlanContext<'_>,
	stale_references: &ReferenceSet,
) -> ReferenceSet {
	let mut references = stale_references.clone();
	for (_, next_reference) in reference_id_remaps(context.impact) {
		if let Some(reference_idx) = context.reference_indexes.get(next_reference) {
			references.insert(*reference_idx);
		}
	}
	references
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
