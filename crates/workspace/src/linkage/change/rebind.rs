use rustc_hash::FxHashMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use code_moniker_core::core::moniker::query::bare_callable_name;
use code_moniker_core::lang::build_manifest::Manifest;
use code_moniker_core::lang::{Lang, kinds};

use crate::linkage::binding::LinkageStore;
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::LinkageQuery;
use crate::linkage::catalog::{ReferenceOrdinal, ReferenceSet, SymbolSet};
use crate::linkage::change::{LinkageRefreshImpact, SymbolDelta};
use crate::linkage::resolve::{matches_any_source, matches_any_symbol};
use crate::path_util::normalize_path;
use crate::snapshot::{RecordTable, ReferenceId, ReferenceRecord, SourceId, SymbolInventoryIndex};
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) struct BindingReadModel<'a> {
	pub(in crate::linkage) store: &'a LinkageStore,
	pub(in crate::linkage) inventory: &'a SymbolInventoryIndex,
	pub(in crate::linkage) reference_indexes: &'a FxHashMap<ReferenceId, ReferenceOrdinal>,
}

pub(in crate::linkage) struct EditedGraph<'a> {
	pub(in crate::linkage) references: &'a RecordTable<ReferenceRecord>,
	pub(in crate::linkage) material: &'a CodeIndexMaterial,
	pub(in crate::linkage) candidates: &'a CandidateCatalog,
}

pub(in crate::linkage) struct RebindScope {
	pub(super) stale_references: ReferenceSet,
	pub(super) target_index_references: ReferenceSet,
	pub(super) changed_files: BTreeSet<usize>,
}

struct EditedSources {
	source_ids: BTreeSet<SourceId>,
	files: BTreeSet<usize>,
}

impl RebindScope {
	pub(in crate::linkage) fn plan(
		bindings: BindingReadModel<'_>,
		graph: EditedGraph<'_>,
		impact: &LinkageRefreshImpact,
	) -> Self {
		let edited_sources = EditedSources::from_impact(graph.material, impact);
		let mut stale_references =
			references_edited_by_change(&bindings, &graph, impact, &edited_sources);
		stale_references.union_with(&references_crossing_changed_manifest_boundaries(
			&bindings, &graph, impact,
		));
		stale_references.union_with(&references_matching_changed_definitions(
			&bindings,
			&graph,
			impact,
			&edited_sources,
		));
		stale_references.union_with(&references_resolved_to_retargeted_targets(
			&bindings,
			&graph,
			impact,
			&edited_sources,
		));
		stale_references.union_with(&references_resolved_to_missing_targets(
			&bindings, &graph, impact,
		));
		let language_sources = crate::linkage::language::binding_invalidation_sources(
			graph.references,
			graph.material,
			impact,
			&edited_sources.source_ids,
			&edited_sources.files,
		);
		stale_references.union_with(&references_in_sources(&graph, &language_sources));
		expand_typed_semantic_dependencies(
			&bindings,
			&graph,
			impact,
			&edited_sources,
			&mut stale_references,
		);
		let target_index_references =
			references_needing_target_index_refresh(&bindings, impact, &stale_references);
		Self {
			stale_references,
			target_index_references,
			changed_files: edited_sources.files,
		}
	}
}

fn expand_typed_semantic_dependencies(
	bindings: &BindingReadModel<'_>,
	graph: &EditedGraph<'_>,
	impact: &LinkageRefreshImpact,
	edited_sources: &EditedSources,
	affected: &mut ReferenceSet,
) {
	let typed_sources = edited_sources
		.source_ids
		.iter()
		.filter(|source| {
			graph.material.files.iter().any(|file| {
				file.source_id == **source && matches!(file.lang, Lang::Python | Lang::Cs)
			})
		})
		.cloned()
		.collect::<BTreeSet<_>>();
	if typed_sources.is_empty() {
		return;
	}

	let semantic_fact_changed = impact.references().removed_semantic_fact()
		|| impact.references().changed_ids().iter().any(|id| {
			graph
				.references
				.iter()
				.find(|reference| reference.id == *id)
				.is_some_and(|reference| {
					matches!(
						reference.kind.as_bytes(),
						kinds::TYPED_AS | kinds::RETURNS_TYPE
					)
				})
		});
	if semantic_fact_changed {
		affected.union_with(&references_in_sources(graph, &typed_sources));
	}
	if semantic_fact_changed
		&& let Some(resolved) = &bindings.store.indexes.resolved_by_target_source
	{
		for (symbol_id, _) in graph.material.symbols() {
			if !graph
				.material
				.symbol_source(&symbol_id)
				.is_some_and(|source| typed_sources.contains(&source))
			{
				continue;
			}
			let Some((symbol, _)) = graph.candidates.candidate_for_symbol_id(&symbol_id) else {
				continue;
			};
			if let Some(references) = resolved.get_symbol(symbol) {
				affected.union_with(references);
			}
		}
	}

	let definitions_changed = !impact.definitions().candidate_ids().is_empty()
		|| !impact.definitions().changed_ids().is_empty()
		|| !impact.definitions().retargeted_identities().is_empty();
	for symbol_id in impact
		.definitions()
		.candidate_ids()
		.iter()
		.chain(impact.definitions().changed_ids())
	{
		let Some(last) = graph
			.material
			.symbol_moniker(symbol_id)
			.and_then(|moniker| moniker.as_view().segments().last())
		else {
			continue;
		};
		if last.kind != kinds::METHOD {
			continue;
		}
		if let Some(references) = bindings
			.store
			.indexes
			.references_by_call_name
			.get(bare_callable_name(last.name))
		{
			affected.union_with(references);
		}
	}
	let source_symbols = affected
		.iter()
		.filter_map(|reference| graph.references.get(reference.index()))
		.filter(|reference| {
			semantic_fact_changed
				|| (definitions_changed
					&& matches!(reference.kind.as_bytes(), b"method_call" | b"calls"))
		})
		.map(|reference| reference.source_symbol)
		.collect::<BTreeSet<_>>();
	for source_symbol in source_symbols {
		if let Some(references) = bindings
			.store
			.indexes
			.references_by_source_symbol
			.get(&source_symbol)
		{
			affected.union_with(references);
		}
	}
}

impl EditedSources {
	fn from_impact(material: &CodeIndexMaterial, impact: &LinkageRefreshImpact) -> Self {
		let source_ids = impact.changed_sources().iter().cloned().collect();
		let files = changed_source_files(material, &source_ids);
		Self { source_ids, files }
	}
}

fn references_edited_by_change(
	bindings: &BindingReadModel<'_>,
	graph: &EditedGraph<'_>,
	impact: &LinkageRefreshImpact,
	edited_sources: &EditedSources,
) -> ReferenceSet {
	if impact.has_precise_graph_diff() {
		return references_for_ids(bindings, impact.references().changed_ids());
	}
	references_in_sources(graph, &edited_sources.source_ids)
}

fn references_crossing_changed_manifest_boundaries(
	bindings: &BindingReadModel<'_>,
	graph: &EditedGraph<'_>,
	impact: &LinkageRefreshImpact,
) -> ReferenceSet {
	let roots = policy_source_roots(graph.material, impact.changed_paths());
	references_in_roots(bindings, &roots)
}

fn references_matching_changed_definitions(
	bindings: &BindingReadModel<'_>,
	graph: &EditedGraph<'_>,
	impact: &LinkageRefreshImpact,
	edited_sources: &EditedSources,
) -> ReferenceSet {
	if impact.has_precise_graph_diff() {
		let symbols = definition_candidates_changed_by_edit(graph, impact);
		return references_matching_symbols(bindings, graph, &symbols);
	}
	references_matching_definitions_in_files(bindings, graph, &edited_sources.files)
}

fn references_resolved_to_retargeted_targets(
	bindings: &BindingReadModel<'_>,
	_graph: &EditedGraph<'_>,
	impact: &LinkageRefreshImpact,
	edited_sources: &EditedSources,
) -> ReferenceSet {
	if impact.has_precise_graph_diff() {
		let identities = impact
			.definitions()
			.retargeted_identities()
			.iter()
			.cloned()
			.collect::<BTreeSet<_>>();
		return references_resolved_to_identities(bindings, &identities);
	}
	references_resolved_to_sources(bindings, &edited_sources.source_ids)
}

fn references_resolved_to_missing_targets(
	bindings: &BindingReadModel<'_>,
	graph: &EditedGraph<'_>,
	impact: &LinkageRefreshImpact,
) -> ReferenceSet {
	if matches!(
		impact.definitions(),
		SymbolDelta::Unchanged | SymbolDelta::AdditiveOnly { .. }
	) {
		return ReferenceSet::new();
	}
	references_for_ids(
		bindings,
		bindings
			.store
			.missing_resolved_references(graph.material, graph.candidates)
			.as_slice(),
	)
}

fn references_for_ids(bindings: &BindingReadModel<'_>, references: &[ReferenceId]) -> ReferenceSet {
	references
		.iter()
		.filter_map(|reference| bindings.reference_indexes.get(reference).copied())
		.collect()
}

fn references_in_sources(graph: &EditedGraph<'_>, sources: &BTreeSet<SourceId>) -> ReferenceSet {
	graph
		.references
		.iter()
		.enumerate()
		.filter(|(_, reference)| sources.contains(&reference.source))
		.map(|(reference_idx, _)| ReferenceOrdinal::from_index(reference_idx))
		.collect()
}

fn references_in_roots(bindings: &BindingReadModel<'_>, roots: &BTreeSet<usize>) -> ReferenceSet {
	let mut references = ReferenceSet::new();
	for root in roots {
		if let Some(root_references) = bindings.store.indexes.references_by_source_root.get(root) {
			references.union_with(root_references);
		}
	}
	references
}

fn definition_candidates_changed_by_edit(
	graph: &EditedGraph<'_>,
	impact: &LinkageRefreshImpact,
) -> SymbolSet {
	let symbols = impact
		.definitions()
		.candidate_ids()
		.iter()
		.filter_map(|symbol| {
			graph
				.candidates
				.candidate_for_symbol_id(symbol)
				.map(|(symbol, _)| symbol)
		})
		.collect::<SymbolSet>();
	if !symbols.is_empty() || !impact.has_precise_graph_diff() {
		return symbols;
	}
	impact
		.definitions()
		.changed_ids()
		.iter()
		.filter_map(|symbol| {
			graph
				.candidates
				.candidate_for_symbol_id(symbol)
				.map(|(symbol, _)| symbol)
		})
		.collect()
}

fn references_matching_symbols(
	bindings: &BindingReadModel<'_>,
	graph: &EditedGraph<'_>,
	symbols: &SymbolSet,
) -> ReferenceSet {
	let mut seen = ReferenceSet::new();
	let mut stale = ReferenceSet::new();
	for (key, key_symbols) in changed_candidates_by_key(graph.candidates, symbols) {
		let Some(ids) = bindings.store.indexes.references_by_name.get(&key) else {
			continue;
		};
		collect_matching_symbol_references(graph, ids, &key_symbols, &mut seen, &mut stale);
	}
	stale
}

fn references_matching_definitions_in_files(
	bindings: &BindingReadModel<'_>,
	graph: &EditedGraph<'_>,
	files: &BTreeSet<usize>,
) -> ReferenceSet {
	let mut seen = ReferenceSet::new();
	let mut stale = ReferenceSet::new();
	for source_file in files {
		let Some(keys) = graph
			.candidates
			.indexes()
			.source_candidate_keys(*source_file)
		else {
			continue;
		};
		for key in keys {
			let Some(ids) = bindings.store.indexes.references_by_name.get(key) else {
				continue;
			};
			collect_matching_source_references(graph, ids, files, &mut seen, &mut stale);
		}
	}
	stale
}

fn collect_matching_symbol_references(
	graph: &EditedGraph<'_>,
	ids: &ReferenceSet,
	symbols: &SymbolSet,
	seen: &mut ReferenceSet,
	stale: &mut ReferenceSet,
) {
	let mut fresh = ids.clone();
	fresh.remove_all(seen);
	seen.union_with(&fresh);
	for reference_ordinal in fresh.iter() {
		let Some(query) = query_for_reference(graph, reference_ordinal) else {
			continue;
		};
		if matches_any_symbol(graph.candidates, &query, symbols) {
			stale.insert(reference_ordinal);
		}
	}
}

fn collect_matching_source_references(
	graph: &EditedGraph<'_>,
	ids: &ReferenceSet,
	files: &BTreeSet<usize>,
	seen: &mut ReferenceSet,
	stale: &mut ReferenceSet,
) {
	let mut fresh = ids.clone();
	fresh.remove_all(seen);
	seen.union_with(&fresh);
	for reference_ordinal in fresh.iter() {
		let Some(query) = query_for_reference(graph, reference_ordinal) else {
			continue;
		};
		if matches_any_source(graph.candidates, &query, files) {
			stale.insert(reference_ordinal);
		}
	}
}

fn query_for_reference<'a>(
	graph: &'a EditedGraph<'a>,
	reference: ReferenceOrdinal,
) -> Option<LinkageQuery<'a>> {
	let record = graph.references.get(reference.index())?;
	LinkageQuery::new(record, graph.material)
}

fn references_resolved_to_identities(
	bindings: &BindingReadModel<'_>,
	identities: &BTreeSet<String>,
) -> ReferenceSet {
	let mut references = ReferenceSet::new();
	let Some(index) = &bindings.store.indexes.resolved_by_target_source else {
		return references;
	};
	for identity in identities {
		let Some(ordinals) = bindings.inventory.facets().symbols_by_identity(identity) else {
			continue;
		};
		for ordinal in ordinals.iter() {
			if let Some(symbol_references) = index.get_symbol(ordinal) {
				references.union_with(symbol_references);
			}
		}
	}
	references
}

fn references_resolved_to_sources(
	bindings: &BindingReadModel<'_>,
	sources: &BTreeSet<SourceId>,
) -> ReferenceSet {
	let mut references = ReferenceSet::new();
	let Some(index) = &bindings.store.indexes.resolved_by_target_source else {
		return references;
	};
	for source in sources {
		if let Some(source_references) = index.get(source) {
			references.union_with(source_references);
		}
	}
	references
}

fn references_needing_target_index_refresh(
	bindings: &BindingReadModel<'_>,
	impact: &LinkageRefreshImpact,
	stale_references: &ReferenceSet,
) -> ReferenceSet {
	let mut references = stale_references.clone();
	for (_, next_reference) in impact.references().id_remaps() {
		if let Some(reference_idx) = bindings.reference_indexes.get(next_reference) {
			references.insert(*reference_idx);
		}
	}
	references
}

fn changed_candidates_by_key(
	candidates: &CandidateCatalog,
	changed_symbols: &SymbolSet,
) -> FxHashMap<Vec<u8>, SymbolSet> {
	let mut symbols_by_key = FxHashMap::default();
	for symbol in changed_symbols.iter() {
		let Some(symbol_keys) = candidates.query_keys_for_symbol(symbol) else {
			continue;
		};
		for key in symbol_keys {
			symbols_by_key
				.entry(key)
				.or_insert_with(SymbolSet::new)
				.insert(symbol);
		}
	}
	symbols_by_key
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
