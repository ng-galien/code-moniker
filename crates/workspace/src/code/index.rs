use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use code_moniker_core::core::moniker::Moniker;
use rayon::prelude::*;

use crate::code::{def_kind, is_navigable_def, last_name, ref_kind};
use crate::environment;
use crate::lines::LineIndex;
use crate::snapshot::{
	CodeIndex, CodeIndexFields, CodeIndexTimings, ReferenceRecord, SourceCatalog, SourceFileRecord,
	SourceFileRecordFields, SourceId, SymbolRecord, SymbolRecordFields, WorkspaceFailure,
	WorkspaceResource, WorkspaceResult,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeIndexRefresh {
	pub index: CodeIndex,
	pub changed_sources: Vec<SourceId>,
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
		refresh_local_code_index(&self.cache, &self.options, current, paths)
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
	let sources = source_records(&material.files);
	let identity_scheme = material.identity.scheme().to_string();
	cache.insert_index(generation, material);
	Ok(CodeIndex::from_fields(CodeIndexFields {
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
	}))
}

fn refresh_local_code_index(
	cache: &LocalResourceCache,
	_options: &LocalCodeIndexOptions,
	current: &CodeIndex,
	paths: &[PathBuf],
) -> WorkspaceResult<CodeIndexRefresh> {
	let total_timer = Instant::now();
	let current_material = cache.index_material(current.generation).ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::CodeIndex,
			"code index material is unavailable",
		)
	})?;
	let mut files = current_material.files.clone();
	let mut changed_sources = Vec::new();
	let extract_timer = Instant::now();
	for path in paths {
		let Some(source) = current_material.source_catalog.resolve_source(path) else {
			continue;
		};
		let Some(file_idx) = source.eager_index else {
			continue;
		};
		let indexed = extract_source_file(
			&current_material.source_catalog,
			file_idx,
			&source.path,
			None,
		)?;
		if let Some(slot) = files.get_mut(file_idx) {
			*slot = indexed.clone();
			push_unique_source(&mut changed_sources, indexed.source_id);
		}
	}
	let extract_sources = extract_timer.elapsed();
	if changed_sources.is_empty() {
		return Ok(CodeIndexRefresh {
			index: current.clone(),
			changed_sources,
		});
	}
	let semantic_timer = Instant::now();
	let material = material_from_files(current_material.source_catalog.clone(), files);
	let sources = source_records(&material.files);
	let symbols = replace_symbols(current, &changed_sources, &material);
	let references = replace_references(current, &changed_sources, &material);
	let semantic_index = semantic_timer.elapsed();
	let generation = cache.next_generation();
	let identity_scheme = material.identity.scheme().to_string();
	cache.insert_index(generation, material);
	Ok(CodeIndexRefresh {
		index: CodeIndex::from_fields(CodeIndexFields {
			generation,
			catalog_generation: current.catalog_generation,
			identity_scheme,
			sources,
			symbols,
			references,
			timings: CodeIndexTimings {
				extract_sources,
				semantic_index,
				total: total_timer.elapsed(),
			},
		}),
		changed_sources,
	})
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
) -> WorkspaceResult<Vec<IndexedSourceFile>> {
	source_material
		.sources
		.files
		.par_iter()
		.enumerate()
		.map(|(file_idx, file)| {
			extract_source_file(source_material, file_idx, &file.path, cache_dir)
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
	files: Vec<IndexedSourceFile>,
) -> (Vec<SymbolRecord>, Vec<ReferenceRecord>, CodeIndexMaterial) {
	let (symbol_count, reference_count) = graph_record_counts(&files);
	let mut symbols = Vec::with_capacity(symbol_count);
	let mut references = Vec::with_capacity(reference_count);
	let mut symbols_by_moniker = rustc_hash::FxHashMap::default();
	symbols_by_moniker.reserve(symbol_count);
	let mut reference_identity_pool = TargetIdentityPool::default();
	let identity = source_material.identity.clone();
	for (file_idx, file) in files.iter().enumerate() {
		let line_index = LineIndex::new(&file.source);
		collect_symbols(
			file_idx,
			file,
			&line_index,
			&mut symbols,
			&mut symbols_by_moniker,
		);
		collect_references(
			file_idx,
			file,
			&line_index,
			&mut references,
			&mut reference_identity_pool,
		);
	}
	symbols_by_moniker.shrink_to_fit();
	let mut files = files;
	files.shrink_to_fit();
	let material = CodeIndexMaterial {
		source_catalog: source_material,
		files,
		identity,
		symbols_by_moniker,
	};
	(symbols, references, material)
}

fn material_from_files(
	source_material: SourceCatalogMaterial,
	mut files: Vec<IndexedSourceFile>,
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

fn replace_symbols(
	current: &CodeIndex,
	changed_sources: &[SourceId],
	material: &CodeIndexMaterial,
) -> Vec<SymbolRecord> {
	let mut symbols = current
		.symbols
		.iter()
		.filter(|symbol| !changed_sources.contains(&symbol.source))
		.cloned()
		.collect::<Vec<_>>();
	let mut changed = records_for_sources(changed_sources, &material.files, collect_file_symbols);
	symbols.append(&mut changed);
	symbols.shrink_to_fit();
	symbols
}

fn replace_references(
	current: &CodeIndex,
	changed_sources: &[SourceId],
	material: &CodeIndexMaterial,
) -> Vec<ReferenceRecord> {
	let mut references = current
		.references
		.iter()
		.filter(|reference| !changed_sources.contains(&reference.source))
		.cloned()
		.collect::<Vec<_>>();
	let mut changed =
		records_for_sources(changed_sources, &material.files, collect_file_references);
	references.append(&mut changed);
	references.shrink_to_fit();
	references
}

fn records_for_sources<T, F>(
	changed_sources: &[SourceId],
	files: &[IndexedSourceFile],
	mut collect: F,
) -> Vec<T>
where
	F: FnMut(usize, &IndexedSourceFile, &LineIndex, &mut Vec<T>),
{
	let mut out = Vec::new();
	for (file_idx, file) in files.iter().enumerate() {
		if !changed_sources.contains(&file.source_id) {
			continue;
		}
		let line_index = LineIndex::new(&file.source);
		collect(file_idx, file, &line_index, &mut out);
	}
	out
}

fn collect_file_symbols(
	file_idx: usize,
	file: &IndexedSourceFile,
	line_index: &LineIndex,
	symbols: &mut Vec<SymbolRecord>,
) {
	let mut symbols_by_moniker = rustc_hash::FxHashMap::default();
	collect_symbols(file_idx, file, line_index, symbols, &mut symbols_by_moniker);
}

fn collect_file_references(
	file_idx: usize,
	file: &IndexedSourceFile,
	line_index: &LineIndex,
	references: &mut Vec<ReferenceRecord>,
) {
	let mut reference_identity_pool = TargetIdentityPool::default();
	collect_references(
		file_idx,
		file,
		line_index,
		references,
		&mut reference_identity_pool,
	);
}

fn graph_record_counts(files: &[IndexedSourceFile]) -> (usize, usize) {
	files.iter().fold((0usize, 0usize), |(defs, refs), file| {
		(
			defs + file.graph.defs().count(),
			refs + file.graph.refs().count(),
		)
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
	symbols_by_moniker: &mut rustc_hash::FxHashMap<Moniker, crate::snapshot::SymbolId>,
) {
	for (def_idx, def) in file.graph.defs().enumerate() {
		let id = file.graph_identity().symbol_id(file_idx, def_idx);
		let parent = def
			.parent
			.map(|parent_idx| file.graph_identity().symbol_id(file_idx, parent_idx));
		symbols_by_moniker.insert(def.moniker.clone(), id.clone());
		symbols.push(SymbolRecord::from_fields(SymbolRecordFields {
			id,
			source: file.source_id.clone(),
			identity: file.graph_identity().moniker_uri(&def.moniker),
			name: last_name(&def.moniker),
			kind: def_kind(def),
			visibility: def_visibility(def),
			signature: String::from_utf8_lossy(&def.signature).to_string(),
			navigable: is_navigable_def(file.lang, def),
			line_range: def
				.position
				.map(|(start, end)| line_index.line_range(start, end)),
			parent,
		}));
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
				id.as_str(),
				file.source_id.clone(),
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

fn source_records(files: &[IndexedSourceFile]) -> Vec<SourceFileRecord> {
	files
		.iter()
		.map(|file| {
			SourceFileRecord::from_fields(SourceFileRecordFields {
				id: file.source_id.clone(),
				uri: file.source_uri.clone(),
				source_root: file.source_root,
				path: file.path.display().to_string(),
				rel_path: file.rel_path.display().to_string(),
				anchor: file.anchor.display().to_string(),
				language: file.lang.tag().to_string(),
				text: String::new(),
			})
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
