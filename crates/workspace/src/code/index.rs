// code-moniker: ignore-file[smell-clone-reflex]
// Code index refresh and graph diffing clone stable IDs into owned snapshots/diffs.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use code_moniker_core::core::moniker::Moniker;
use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::code::{def_kind, is_navigable_def, last_name, ref_kind};
use crate::environment;
use crate::lines::LineIndex;
use crate::snapshot::{
	CodeIndex, CodeIndexTimings, RecordTable, ReferenceId, ReferenceRecord, SourceCatalog,
	SourceFileRecord, SourceId, SymbolId, SymbolRecord, WorkspaceFailure, WorkspaceResource,
	WorkspaceResult,
};
use crate::source::{
	CodeIndexMaterial, IndexedSourceFile, LocalResourceCache, SourceCatalogMaterial,
};

use crate::source::LocalIdentityResolver;

pub trait CodeIndexPort {
	fn build_index(&mut self, catalog: &SourceCatalog) -> WorkspaceResult<CodeIndex>;
	fn refresh_paths(
		&mut self,
		current: &CodeIndex,
		paths: &[PathBuf],
	) -> WorkspaceResult<CodeIndexRefresh>;
	fn refresh_catalog_paths(
		&mut self,
		current: &CodeIndex,
		catalog: &SourceCatalog,
		paths: &[PathBuf],
	) -> WorkspaceResult<CodeIndexRefresh>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeIndexRefresh {
	pub index: CodeIndex,
	pub changed_sources: Vec<SourceId>,
	pub graph_diff: CodeIndexGraphDiff,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodeIndexGraphDiff {
	pub added_symbols: Vec<SymbolId>,
	pub modified_symbols: Vec<SymbolId>,
	pub changed_symbols: Vec<SymbolId>,
	pub removed_symbols: Vec<SymbolId>,
	pub modified_symbol_identities: Vec<String>,
	pub removed_symbol_identities: Vec<String>,
	pub changed_references: Vec<ReferenceId>,
	pub removed_references: Vec<ReferenceId>,
	pub symbol_id_remaps: Vec<(SymbolId, SymbolId)>,
	pub reference_id_remaps: Vec<(ReferenceId, ReferenceId)>,
	pub unchanged_symbols: usize,
	pub unchanged_references: usize,
}

impl CodeIndexGraphDiff {
	pub fn changed_symbol_count(&self) -> usize {
		self.changed_symbols.len() + self.removed_symbols.len()
	}

	pub fn changed_reference_count(&self) -> usize {
		self.changed_references.len() + self.removed_references.len()
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocalCodeIndexOptions {
	pub cache_dir: Option<PathBuf>,
}

impl LocalCodeIndexOptions {
	pub fn new(cache_dir: Option<PathBuf>) -> Self {
		Self { cache_dir }
	}
}

pub struct LocalCodeIndex {
	options: LocalCodeIndexOptions,
	cache: LocalResourceCache,
}

impl LocalCodeIndex {
	pub fn new(options: LocalCodeIndexOptions, cache: LocalResourceCache) -> Self {
		Self { options, cache }
	}
}

impl CodeIndexPort for LocalCodeIndex {
	fn build_index(&mut self, catalog: &SourceCatalog) -> WorkspaceResult<CodeIndex> {
		build_local_code_index(&self.cache, &self.options, catalog)
	}

	fn refresh_paths(
		&mut self,
		current: &CodeIndex,
		paths: &[PathBuf],
	) -> WorkspaceResult<CodeIndexRefresh> {
		refresh_local_code_index(&self.cache, &self.options, current, None, paths)
	}

	fn refresh_catalog_paths(
		&mut self,
		current: &CodeIndex,
		catalog: &SourceCatalog,
		paths: &[PathBuf],
	) -> WorkspaceResult<CodeIndexRefresh> {
		refresh_local_code_index(&self.cache, &self.options, current, Some(catalog), paths)
	}
}

fn build_local_code_index(
	cache: &LocalResourceCache,
	options: &LocalCodeIndexOptions,
	catalog: &SourceCatalog,
) -> WorkspaceResult<CodeIndex> {
	let total_timer = Instant::now();
	let source_material = source_material(cache, catalog)?;
	let generation = cache.next_generation();
	let extract_timer = Instant::now();
	let files = extract_source_files(&source_material, options.cache_dir.as_deref())?;
	let extract_sources = extract_timer.elapsed();
	let semantic_timer = Instant::now();
	let (symbols, references, material) = build_semantic_index(source_material, files);
	let semantic_index = semantic_timer.elapsed();
	let mut sources = source_records(&material.files);
	let identity_scheme = material.identity.scheme().to_string();
	cache.insert_index(generation, material);
	sources.shrink_to_fit();
	Ok(CodeIndex {
		generation,
		catalog_generation: catalog.generation,
		identity_scheme,
		sources,
		symbols,
		references,
		timings: CodeIndexTimings {
			extract_sources,
			semantic_index,
			total: total_timer.elapsed(),
		},
	})
}

fn refresh_local_code_index(
	cache: &LocalResourceCache,
	options: &LocalCodeIndexOptions,
	current: &CodeIndex,
	extended_catalog: Option<&SourceCatalog>,
	paths: &[PathBuf],
) -> WorkspaceResult<CodeIndexRefresh> {
	let total_timer = Instant::now();
	let current_material = cache.index_material(current.generation).ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			"code index material is unavailable",
		)
	})?;
	let source_catalog = match extended_catalog {
		Some(catalog) => cache.source_material(catalog.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::CodeIndex,
				"extended source catalog material is unavailable",
			)
		})?,
		None => current_material.source_catalog.clone(),
	};
	let mut files = current_material.files.clone();
	let mut changed_sources = Vec::new();
	let mut changed_file_indexes = BTreeSet::new();
	let extract_timer = Instant::now();
	refresh_retired_slots(RetiredSlotRefresh {
		previous_catalog: &current_material.source_catalog,
		source_catalog: &source_catalog,
		cache_dir: options.cache_dir.as_deref(),
		files: &mut files,
		changed_sources: &mut changed_sources,
		changed_file_indexes: &mut changed_file_indexes,
	})?;
	for file_idx in files.len()..source_catalog.sources.files.len() {
		let file = &source_catalog.sources.files[file_idx];
		let indexed = extract_source_file(
			&source_catalog,
			file_idx,
			&file.path.clone(),
			options.cache_dir.as_deref(),
		)?;
		push_unique_source(&mut changed_sources, indexed.source_id);
		changed_file_indexes.insert(file_idx);
		files.push(Arc::new(indexed));
	}
	for path in paths {
		let Some(source) = source_catalog.resolve_source(path) else {
			continue;
		};
		let Some(file_idx) = source.eager_index else {
			continue;
		};
		if changed_file_indexes.contains(&file_idx) {
			continue;
		}
		let indexed = extract_source_file(
			&source_catalog,
			file_idx,
			&source.path,
			options.cache_dir.as_deref(),
		)?;
		if let Some(slot) = files.get_mut(file_idx) {
			push_unique_source(&mut changed_sources, indexed.source_id);
			changed_file_indexes.insert(file_idx);
			*slot = Arc::new(indexed);
		}
	}
	let extract_sources = extract_timer.elapsed();
	if changed_sources.is_empty() {
		return Ok(CodeIndexRefresh {
			index: current.clone(),
			changed_sources,
			graph_diff: CodeIndexGraphDiff::default(),
		});
	}
	let semantic_timer = Instant::now();
	let material = material_from_files(source_catalog, files);
	let sources = source_records(&material.files);
	let graph_diff = graph_diff(current_material.as_ref(), &material, &changed_file_indexes);
	let mut symbols = current.symbols.clone();
	let mut references = current.references.clone();
	for file_idx in &changed_file_indexes {
		let (file_symbols, file_references) =
			records_for_file(*file_idx, &material.files[*file_idx]);
		symbols.replace(*file_idx, Arc::from(file_symbols));
		references.replace(*file_idx, Arc::from(file_references));
	}
	let semantic_index = semantic_timer.elapsed();
	let generation = cache.next_generation();
	let identity_scheme = material.identity.scheme().to_string();
	cache.insert_index(generation, material);
	Ok(CodeIndexRefresh {
		index: CodeIndex {
			generation,
			catalog_generation: extended_catalog
				.map(|catalog| catalog.generation)
				.unwrap_or(current.catalog_generation),
			identity_scheme,
			sources,
			symbols,
			references,
			timings: CodeIndexTimings {
				extract_sources,
				semantic_index,
				total: total_timer.elapsed(),
			},
		},
		changed_sources,
		graph_diff,
	})
}

struct RetiredSlotRefresh<'a> {
	previous_catalog: &'a SourceCatalogMaterial,
	source_catalog: &'a SourceCatalogMaterial,
	cache_dir: Option<&'a Path>,
	files: &'a mut Vec<Arc<IndexedSourceFile>>,
	changed_sources: &'a mut Vec<SourceId>,
	changed_file_indexes: &'a mut BTreeSet<usize>,
}

fn refresh_retired_slots(refresh: RetiredSlotRefresh<'_>) -> WorkspaceResult<()> {
	let slots = refresh
		.files
		.len()
		.min(refresh.source_catalog.sources.files.len());
	for file_idx in 0..slots {
		let was_retired = refresh.previous_catalog.sources.files[file_idx].retired;
		let is_retired = refresh.source_catalog.sources.files[file_idx].retired;
		if was_retired == is_retired {
			continue;
		}
		let indexed = if is_retired {
			tombstone_file(&refresh.files[file_idx])
		} else {
			extract_source_file(
				refresh.source_catalog,
				file_idx,
				&refresh.source_catalog.sources.files[file_idx].path.clone(),
				refresh.cache_dir,
			)?
		};
		push_unique_source(refresh.changed_sources, indexed.source_id);
		refresh.changed_file_indexes.insert(file_idx);
		refresh.files[file_idx] = Arc::new(indexed);
	}
	Ok(())
}

fn tombstone_file(previous: &IndexedSourceFile) -> IndexedSourceFile {
	IndexedSourceFile {
		source_root: previous.source_root,
		source_id: previous.source_id,
		source_uri: previous.source_uri.clone(),
		identity: previous.identity.clone(),
		path: previous.path.clone(),
		rel_path: previous.rel_path.clone(),
		anchor: previous.anchor.clone(),
		lang: previous.lang,
		graph: code_moniker_core::core::code_graph::CodeGraph::from_records(Vec::new(), Vec::new()),
		source: String::new(),
	}
}

fn source_material(
	cache: &LocalResourceCache,
	catalog: &SourceCatalog,
) -> WorkspaceResult<SourceCatalogMaterial> {
	cache.source_material(catalog.generation).ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			"source catalog material is unavailable",
		)
	})
}

fn extract_source_files(
	source_material: &SourceCatalogMaterial,
	cache_dir: Option<&std::path::Path>,
) -> WorkspaceResult<Vec<Arc<IndexedSourceFile>>> {
	source_material
		.sources
		.files
		.par_iter()
		.enumerate()
		.map(|(file_idx, file)| {
			extract_source_file(source_material, file_idx, &file.path, cache_dir).map(Arc::new)
		})
		.collect()
}

fn extract_source_file(
	source_material: &SourceCatalogMaterial,
	file_idx: usize,
	path: &Path,
	cache_dir: Option<&Path>,
) -> WorkspaceResult<IndexedSourceFile> {
	let file = source_material.sources.files.get(file_idx).ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			format!("source file index {file_idx} is unavailable"),
		)
	})?;
	let ctx = &source_material.sources.roots[file.source].ctx;
	let (graph, extracted_source) =
		environment::load_or_extract_source(path, &file.anchor, file.lang, cache_dir, ctx)
			.map_err(|err| {
				WorkspaceFailure::new(
					WorkspaceResource::CodeIndex,
					format!("cannot extract {}: {err}", path.display()),
				)
			})?;
	let source = match extracted_source {
		Some(source) => source,
		None => std::fs::read_to_string(path).map_err(|err| {
			WorkspaceFailure::new(
				WorkspaceResource::CodeIndex,
				format!("cannot read {}: {err}", path.display()),
			)
		})?,
	};
	Ok(IndexedSourceFile {
		source_root: file.source,
		source_id: source_material
			.source_id_for_file(file_idx)
			.ok_or_else(|| {
				WorkspaceFailure::new(
					WorkspaceResource::CodeIndex,
					format!("source id is unavailable for {}", file.rel_path.display()),
				)
			})?,
		source_uri: source_material
			.source_uri_for_path(&file.path)
			.ok_or_else(|| {
				WorkspaceFailure::new(
					WorkspaceResource::CodeIndex,
					format!("source uri is unavailable for {}", file.path.display()),
				)
			})?,
		identity: source_material.identity.clone(),
		path: file.path.clone(),
		rel_path: file.rel_path.clone(),
		anchor: file.anchor.clone(),
		lang: file.lang,
		graph,
		source,
	})
}

fn build_semantic_index(
	source_material: SourceCatalogMaterial,
	files: Vec<Arc<IndexedSourceFile>>,
) -> (
	RecordTable<SymbolRecord>,
	RecordTable<ReferenceRecord>,
	CodeIndexMaterial,
) {
	let mut symbol_shards = Vec::with_capacity(files.len());
	let mut reference_shards = Vec::with_capacity(files.len());
	for (file_idx, file) in files.iter().enumerate() {
		let (symbols, references) = records_for_file(file_idx, file);
		symbol_shards.push(Arc::from(symbols));
		reference_shards.push(Arc::from(references));
	}
	let material = material_from_files(source_material, files);
	(
		RecordTable::from_shards(symbol_shards),
		RecordTable::from_shards(reference_shards),
		material,
	)
}

fn material_from_files(
	source_material: SourceCatalogMaterial,
	mut files: Vec<Arc<IndexedSourceFile>>,
) -> CodeIndexMaterial {
	let symbol_count = files.iter().map(|file| file.graph.def_count()).sum();
	let mut symbols_by_moniker = rustc_hash::FxHashMap::default();
	symbols_by_moniker.reserve(symbol_count);
	for (file_idx, file) in files.iter().enumerate() {
		for (def_idx, def) in file.graph.defs().enumerate() {
			symbols_by_moniker.insert(
				def.moniker.clone(),
				file.graph_identity().symbol_id(file_idx, def_idx),
			);
		}
	}
	symbols_by_moniker.shrink_to_fit();
	files.shrink_to_fit();
	let identity = source_material.identity.clone();
	CodeIndexMaterial {
		source_catalog: source_material,
		files,
		identity,
		symbols_by_moniker,
	}
}

fn graph_diff(
	previous: &CodeIndexMaterial,
	next: &CodeIndexMaterial,
	changed_files: &BTreeSet<usize>,
) -> CodeIndexGraphDiff {
	let mut diff = CodeIndexGraphDiff::default();
	for file_idx in changed_files {
		let Some(next_file) = next.files.get(*file_idx) else {
			continue;
		};
		let (previous_symbols, previous_references) = match previous.files.get(*file_idx) {
			Some(previous_file) => records_for_file(*file_idx, previous_file),
			None => (Vec::new(), Vec::new()),
		};
		let (next_symbols, next_references) = records_for_file(*file_idx, next_file);
		diff_symbols(&previous_symbols, &next_symbols, &mut diff);
		diff_references(
			&previous_references,
			previous,
			&next_references,
			next,
			&mut diff,
		);
	}
	diff
}

fn records_for_file(
	file_idx: usize,
	file: &IndexedSourceFile,
) -> (Vec<SymbolRecord>, Vec<ReferenceRecord>) {
	let line_index = LineIndex::new(&file.source);
	let mut symbols = Vec::with_capacity(file.graph.def_count());
	collect_symbols(file_idx, file, &line_index, &mut symbols);
	let mut reference_identity_pool = TargetIdentityPool::default();
	let mut references = Vec::with_capacity(file.graph.ref_count());
	collect_references(
		file_idx,
		file,
		&line_index,
		&mut references,
		&mut reference_identity_pool,
	);
	(symbols, references)
}

fn diff_symbols(previous: &[SymbolRecord], next: &[SymbolRecord], diff: &mut CodeIndexGraphDiff) {
	let mut next_by_key = symbol_record_indexes(next);
	for previous_symbol in previous {
		let key = symbol_key(previous_symbol);
		let Some(next_idx) = pop_index(&mut next_by_key, &key) else {
			diff.removed_symbols.push(previous_symbol.id);
			diff.removed_symbol_identities
				.push(previous_symbol.identity.to_string());
			continue;
		};
		let next_symbol = &next[next_idx];
		if symbol_linkage_fields_changed(previous_symbol, next_symbol) {
			diff.modified_symbols.push(next_symbol.id);
			diff.modified_symbol_identities
				.push(next_symbol.identity.to_string());
			diff.changed_symbols.push(next_symbol.id);
			continue;
		}
		if previous_symbol.id != next_symbol.id {
			diff.symbol_id_remaps
				.push((previous_symbol.id, next_symbol.id));
		}
		diff.unchanged_symbols += 1;
	}
	for indexes in next_by_key.into_values() {
		for idx in indexes {
			diff.added_symbols.push(next[idx].id);
			diff.changed_symbols.push(next[idx].id);
		}
	}
}

fn diff_references(
	previous: &[ReferenceRecord],
	previous_material: &CodeIndexMaterial,
	next: &[ReferenceRecord],
	next_material: &CodeIndexMaterial,
	diff: &mut CodeIndexGraphDiff,
) {
	let mut next_by_key = reference_record_indexes(next, next_material);
	for previous_reference in previous {
		let Some(key) = reference_key(previous_reference, previous_material) else {
			diff.removed_references.push(previous_reference.id);
			continue;
		};
		let Some(next_idx) = pop_index(&mut next_by_key, &key) else {
			diff.removed_references.push(previous_reference.id);
			continue;
		};
		let next_reference = &next[next_idx];
		if previous_reference.id != next_reference.id {
			diff.reference_id_remaps
				.push((previous_reference.id, next_reference.id));
		}
		diff.unchanged_references += 1;
	}
	for indexes in next_by_key.into_values() {
		for idx in indexes {
			diff.changed_references.push(next[idx].id);
		}
	}
}

fn symbol_record_indexes(records: &[SymbolRecord]) -> FxHashMap<Arc<str>, Vec<usize>> {
	let mut by_key = FxHashMap::<Arc<str>, Vec<usize>>::default();
	for (idx, record) in records.iter().enumerate() {
		by_key.entry(symbol_key(record)).or_default().push(idx);
	}
	by_key
}

fn reference_record_indexes(
	records: &[ReferenceRecord],
	material: &CodeIndexMaterial,
) -> FxHashMap<ReferenceKey, Vec<usize>> {
	let mut by_key = FxHashMap::<ReferenceKey, Vec<usize>>::default();
	for (idx, record) in records.iter().enumerate() {
		if let Some(key) = reference_key(record, material) {
			by_key.entry(key).or_default().push(idx);
		}
	}
	by_key
}

fn pop_index<K: Eq + std::hash::Hash>(
	by_key: &mut FxHashMap<K, Vec<usize>>,
	key: &K,
) -> Option<usize> {
	let indexes = by_key.get_mut(key)?;
	let idx = indexes.remove(0);
	if indexes.is_empty() {
		by_key.remove(key);
	}
	Some(idx)
}

fn symbol_key(symbol: &SymbolRecord) -> Arc<str> {
	Arc::clone(&symbol.identity)
}

fn symbol_linkage_fields_changed(previous: &SymbolRecord, next: &SymbolRecord) -> bool {
	previous.identity != next.identity
		|| previous.name != next.name
		|| previous.kind != next.kind
		|| previous.visibility != next.visibility
		|| previous.signature != next.signature
		|| previous.navigable != next.navigable
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ReferenceKey {
	source_symbol_identity: String,
	target_identity: String,
	kind: String,
	call_name: Option<String>,
	call_arity: Option<usize>,
	confidence: Option<String>,
	receiver: Option<String>,
	alias: Option<String>,
}

fn reference_key(
	reference: &ReferenceRecord,
	material: &CodeIndexMaterial,
) -> Option<ReferenceKey> {
	let source_symbol_identity = material
		.symbol_moniker(&reference.source_symbol)
		.map(|moniker| material.identity.moniker_uri(moniker))?;
	Some(ReferenceKey {
		source_symbol_identity,
		target_identity: reference.target_identity.to_string(),
		kind: reference.kind.clone(),
		call_name: reference.call_name.clone(),
		call_arity: reference.call_arity,
		confidence: reference.confidence.clone(),
		receiver: reference.receiver.clone(),
		alias: reference.alias.clone(),
	})
}

fn push_unique_source(sources: &mut Vec<SourceId>, source: SourceId) {
	if !sources.iter().any(|existing| existing == &source) {
		sources.push(source);
	}
}

fn collect_symbols(
	file_idx: usize,
	file: &IndexedSourceFile,
	line_index: &LineIndex,
	symbols: &mut Vec<SymbolRecord>,
) {
	for (def_idx, def) in file.graph.defs().enumerate() {
		let id = file.graph_identity().symbol_id(file_idx, def_idx);
		let parent = def
			.parent
			.map(|parent_idx| file.graph_identity().symbol_id(file_idx, parent_idx));
		symbols.push(SymbolRecord {
			id,
			source: file.source_id,
			identity: Arc::from(file.graph_identity().moniker_uri(&def.moniker)),
			name: last_name(&def.moniker),
			kind: def_kind(def),
			visibility: def_visibility(def),
			signature: String::from_utf8_lossy(&def.signature).to_string(),
			navigable: is_navigable_def(file.lang, def),
			line_range: def
				.position
				.map(|(start, end)| line_index.line_range(start, end)),
			parent,
		});
	}
}

fn def_visibility(def: &code_moniker_core::core::code_graph::DefRecord) -> String {
	std::str::from_utf8(&def.visibility)
		.unwrap_or("")
		.to_string()
}

fn collect_references(
	file_idx: usize,
	file: &IndexedSourceFile,
	line_index: &LineIndex,
	references: &mut Vec<ReferenceRecord>,
	reference_identity_pool: &mut TargetIdentityPool,
) {
	for (ref_idx, reference) in file.graph.refs().enumerate() {
		let id = file.graph_identity().reference_id(file_idx, ref_idx);
		let source_symbol = file.graph_identity().symbol_id(file_idx, reference.source);
		let target_identity =
			reference_identity_pool.intern(file.graph_identity(), &reference.target);
		references.push(
			ReferenceRecord::new(
				id,
				file.source_id,
				source_symbol,
				target_identity,
				ref_kind(reference),
				reference
					.position
					.map(|(start, end)| line_index.line_range(start, end)),
			)
			.with_call_metadata(ref_attr(&reference.call_name), reference.call_arity)
			.with_metadata(
				ref_attr(&reference.confidence),
				ref_attr(&reference.receiver_hint),
				ref_attr(&reference.alias),
			),
		);
	}
}

#[derive(Default)]
struct TargetIdentityPool {
	values: rustc_hash::FxHashMap<Moniker, Arc<str>>,
}

impl TargetIdentityPool {
	fn intern(&mut self, identity: &LocalIdentityResolver, target: &Moniker) -> Arc<str> {
		if let Some(existing) = self.values.get(target) {
			return Arc::clone(existing);
		}
		let shared = Arc::<str>::from(identity.moniker_uri(target));
		self.values.insert(target.clone(), Arc::clone(&shared));
		shared
	}
}

fn source_records(files: &[Arc<IndexedSourceFile>]) -> Vec<SourceFileRecord> {
	files
		.iter()
		.map(|file| SourceFileRecord {
			id: file.source_id,
			uri: file.source_uri.clone(),
			source_root: file.source_root,
			path: file.path.display().to_string(),
			rel_path: file.rel_path.display().to_string(),
			anchor: file.anchor.display().to_string(),
			language: file.lang.tag().to_string(),
			text: String::new(),
		})
		.collect()
}

fn ref_attr(bytes: &[u8]) -> Option<String> {
	if bytes.is_empty() {
		return None;
	}
	std::str::from_utf8(bytes)
		.ok()
		.filter(|value| !value.is_empty())
		.map(ToOwned::to_owned)
}

trait IndexedSourceIdentity {
	fn graph_identity(&self) -> &LocalIdentityResolver;
}

impl IndexedSourceIdentity for IndexedSourceFile {
	fn graph_identity(&self) -> &LocalIdentityResolver {
		&self.identity
	}
}
