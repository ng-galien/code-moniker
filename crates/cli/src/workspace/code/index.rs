use std::path::PathBuf;

use code_moniker_core::core::moniker::Moniker;

use crate::cache;
use crate::lines::line_range;
use crate::workspace::code::{def_kind, is_navigable_def, last_name, ref_kind};
use crate::workspace::snapshot::{
	CodeIndex, CodeIndexFields, ReferenceRecord, SourceCatalog, SourceFileRecord,
	SourceFileRecordFields, SymbolRecord, SymbolRecordFields, WorkspaceFailure, WorkspaceResource,
	WorkspaceResult,
};
use crate::workspace::source::{
	CodeIndexMaterial, IndexedSourceFile, LocalResourceCache, SourceCatalogMaterial,
};

use crate::workspace::source::LocalIdentityResolver;

pub trait CodeIndexPort {
	fn build_index(&mut self, catalog: &SourceCatalog) -> WorkspaceResult<CodeIndex>;
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
		let source_material = source_material(&self.cache, catalog)?;
		let generation = self.cache.next_generation();
		let files = extract_source_files(&source_material, self.options.cache_dir.as_deref())?;
		let (symbols, references, material) = build_semantic_index(source_material, files);
		let sources = source_records(&material.files);
		let identity_scheme = material.identity.scheme().to_string();
		self.cache.insert_index(generation, material);
		Ok(CodeIndex::from_fields(CodeIndexFields {
			generation,
			catalog_generation: catalog.generation,
			identity_scheme,
			sources,
			symbols,
			references,
		}))
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
) -> WorkspaceResult<Vec<IndexedSourceFile>> {
	let mut files = Vec::new();
	for (file_idx, file) in source_material.sources.files.iter().enumerate() {
		let ctx = &source_material.sources.roots[file.source].ctx;
		let (graph, extracted_source) =
			cache::load_or_extract_result(&file.path, &file.anchor, file.lang, cache_dir, ctx)
				.map_err(|err| {
					WorkspaceFailure::new(
						WorkspaceResource::CodeIndex,
						format!("cannot extract {}: {err}", file.path.display()),
					)
				})?;
		let source = match extracted_source {
			Some(source) => source,
			None => std::fs::read_to_string(&file.path).map_err(|err| {
				WorkspaceFailure::new(
					WorkspaceResource::CodeIndex,
					format!("cannot read {}: {err}", file.path.display()),
				)
			})?,
		};
		files.push(IndexedSourceFile {
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
		});
	}
	Ok(files)
}

fn build_semantic_index(
	source_material: SourceCatalogMaterial,
	files: Vec<IndexedSourceFile>,
) -> (Vec<SymbolRecord>, Vec<ReferenceRecord>, CodeIndexMaterial) {
	let mut symbols = Vec::new();
	let mut references = Vec::new();
	let mut symbols_by_moniker = rustc_hash::FxHashMap::default();
	let mut symbol_monikers = rustc_hash::FxHashMap::default();
	let mut reference_targets = rustc_hash::FxHashMap::default();
	let identity = source_material.identity.clone();
	for (file_idx, file) in files.iter().enumerate() {
		collect_symbols(
			file_idx,
			file,
			&mut symbols,
			&mut symbols_by_moniker,
			&mut symbol_monikers,
		);
		collect_references(file_idx, file, &mut references, &mut reference_targets);
	}
	let material = CodeIndexMaterial {
		source_catalog: source_material,
		files,
		identity,
		symbols_by_moniker,
		symbol_monikers,
		reference_targets,
	};
	(symbols, references, material)
}

fn collect_symbols(
	file_idx: usize,
	file: &IndexedSourceFile,
	symbols: &mut Vec<SymbolRecord>,
	symbols_by_moniker: &mut rustc_hash::FxHashMap<Moniker, crate::workspace::snapshot::SymbolId>,
	symbol_monikers: &mut rustc_hash::FxHashMap<crate::workspace::snapshot::SymbolId, Moniker>,
) {
	for (def_idx, def) in file.graph.defs().enumerate() {
		let id = file.graph_identity().symbol_id(file_idx, def_idx);
		let parent = def
			.parent
			.map(|parent_idx| file.graph_identity().symbol_id(file_idx, parent_idx));
		symbols_by_moniker.insert(def.moniker.clone(), id.clone());
		symbol_monikers.insert(id.clone(), def.moniker.clone());
		symbols.push(SymbolRecord::from_fields(SymbolRecordFields {
			id,
			source: file.source_id.clone(),
			identity: file.graph_identity().moniker_uri(&def.moniker),
			name: last_name(&def.moniker),
			kind: def_kind(def),
			signature: String::from_utf8_lossy(&def.signature).to_string(),
			navigable: is_navigable_def(file.lang, def),
			line_range: def
				.position
				.map(|(start, end)| line_range(&file.source, start, end)),
			parent,
		}));
	}
}

fn collect_references(
	file_idx: usize,
	file: &IndexedSourceFile,
	references: &mut Vec<ReferenceRecord>,
	reference_targets: &mut rustc_hash::FxHashMap<crate::workspace::snapshot::ReferenceId, Moniker>,
) {
	for (ref_idx, reference) in file.graph.refs().enumerate() {
		let id = file.graph_identity().reference_id(file_idx, ref_idx);
		let source_symbol = file.graph_identity().symbol_id(file_idx, reference.source);
		reference_targets.insert(id.clone(), reference.target.clone());
		references.push(
			ReferenceRecord::new(
				id.as_str(),
				file.source_id.clone(),
				source_symbol,
				file.graph_identity().moniker_uri(&reference.target),
				ref_kind(reference),
				reference
					.position
					.map(|(start, end)| line_range(&file.source, start, end)),
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
				text: file.source.clone(),
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
