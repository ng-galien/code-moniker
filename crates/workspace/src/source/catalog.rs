use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::environment;
use crate::snapshot::{
	SourceCatalog, SourceUnit, WorkspaceCancellation, WorkspaceFailure, WorkspaceRequest,
	WorkspaceResource, WorkspaceResult,
};
use crate::sources::{SourceFile, SourceRoot};

use super::content::{
	CachedMemorySource, LocalResourceCache, MEMORY_SOURCE_ROOT, MEMORY_SOURCE_ROOT_LABEL,
	MemorySourceDocument, MemorySourceSet, SourceCatalogMaterial, is_memory_source_path,
	memory_source_path,
};
use super::identity::LocalIdentityResolver;

pub trait SourceCatalogPort {
	fn load_catalog(&mut self, request: &WorkspaceRequest) -> WorkspaceResult<SourceCatalog>;
	fn load_catalog_cancellable(
		&mut self,
		request: &WorkspaceRequest,
		cancellation: &WorkspaceCancellation,
	) -> WorkspaceResult<SourceCatalog> {
		cancellation.check(WorkspaceResource::SourceCatalog)?;
		let catalog = self.load_catalog(request)?;
		cancellation.check(WorkspaceResource::SourceCatalog)?;
		Ok(catalog)
	}

	fn extend_catalog(
		&mut self,
		current: &SourceCatalog,
		paths: &[PathBuf],
	) -> WorkspaceResult<Option<SourceCatalog>>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalSourceCatalogOptions {
	pub paths: Vec<PathBuf>,
	pub files: Option<Vec<PathBuf>>,
	pub project: Option<String>,
	pub identity: LocalIdentityResolver,
}

impl LocalSourceCatalogOptions {
	pub fn new(paths: Vec<PathBuf>, project: Option<String>) -> Self {
		Self {
			paths,
			files: None,
			project,
			identity: LocalIdentityResolver::default(),
		}
	}

	pub fn with_files(mut self, files: Vec<PathBuf>) -> Self {
		self.files = Some(files);
		self
	}

	pub fn with_identity(mut self, identity: LocalIdentityResolver) -> Self {
		self.identity = identity;
		self
	}
}

pub struct LocalSourceCatalog {
	options: LocalSourceCatalogOptions,
	cache: LocalResourceCache,
}

impl LocalSourceCatalog {
	pub fn new(options: LocalSourceCatalogOptions, cache: LocalResourceCache) -> Self {
		Self { options, cache }
	}
}

impl SourceCatalogPort for LocalSourceCatalog {
	fn load_catalog(&mut self, _request: &WorkspaceRequest) -> WorkspaceResult<SourceCatalog> {
		load_local_catalog(self, &WorkspaceCancellation::default())
	}

	fn load_catalog_cancellable(
		&mut self,
		_request: &WorkspaceRequest,
		cancellation: &WorkspaceCancellation,
	) -> WorkspaceResult<SourceCatalog> {
		load_local_catalog(self, cancellation)
	}

	fn extend_catalog(
		&mut self,
		current: &SourceCatalog,
		paths: &[PathBuf],
	) -> WorkspaceResult<Option<SourceCatalog>> {
		extend_local_catalog(&self.cache, current, paths)
	}
}

fn load_local_catalog(
	catalog: &mut LocalSourceCatalog,
	cancellation: &WorkspaceCancellation,
) -> WorkspaceResult<SourceCatalog> {
	let sources = if let Some(files) = &catalog.options.files {
		let [root] = catalog.options.paths.as_slice() else {
			return Err(WorkspaceFailure::new(
				WorkspaceResource::SourceCatalog,
				"explicit source files require exactly one source root",
			));
		};
		environment::discover_source_files(root, files, catalog.options.project.clone())
	} else {
		crate::sources::discover_cancellable(
			&catalog.options.paths,
			catalog.options.project.clone(),
			cancellation,
		)
	}
	.map_err(|err| WorkspaceFailure::new(WorkspaceResource::SourceCatalog, format!("{err:#}")))?;
	cancellation.check(WorkspaceResource::SourceCatalog)?;
	let mut material = SourceCatalogMaterial {
		sources,
		identity: catalog.options.identity.clone(),
		memory_sources: BTreeMap::new(),
		memory_slots: BTreeSet::new(),
		memory_revisions: BTreeMap::new(),
	};
	sync_memory_source_sets(&mut material, &catalog.cache.memory_source_sets());
	let generation = catalog.cache.next_generation();
	let units = catalog_units(&material);
	catalog.cache.insert_sources(generation, material);
	Ok(SourceCatalog::new(generation, units))
}

fn extend_local_catalog(
	cache: &LocalResourceCache,
	current: &SourceCatalog,
	paths: &[PathBuf],
) -> WorkspaceResult<Option<SourceCatalog>> {
	let Some(mut material) = cache.source_material(current.generation) else {
		return Ok(None);
	};
	let added = new_source_files(&material, paths);
	let flipped = flip_retired_slots(&mut material, paths);
	let memory_changed = sync_memory_source_paths(
		&mut material,
		&cache.memory_source_entries(paths),
		cache.memory_source_revisions(),
		paths,
	);
	if added.is_empty() && !flipped && !memory_changed {
		return Ok(None);
	}
	material.sources.files.extend(added);
	let generation = cache.next_generation();
	let units = catalog_units(&material);
	cache.insert_sources(generation, material);
	Ok(Some(SourceCatalog::new(generation, units)))
}

fn flip_retired_slots(material: &mut SourceCatalogMaterial, paths: &[PathBuf]) -> bool {
	let mut flipped = false;
	for path in paths {
		let file_idx = material
			.normalized_file_index(path)
			.or_else(|| material.normalized_file_index(&canonical_lookup_path(path)));
		let Some(file_idx) = file_idx else {
			continue;
		};
		if material.is_memory_slot(&material.sources.files[file_idx].path) {
			continue;
		}
		let exists = material.sources.files[file_idx].path.is_file();
		let file = &mut material.sources.files[file_idx];
		if file.retired != exists {
			continue;
		}
		file.retired = !exists;
		flipped = true;
	}
	flipped
}

fn sync_memory_source_sets(
	material: &mut SourceCatalogMaterial,
	source_sets: &BTreeMap<String, MemorySourceSet>,
) -> bool {
	let previous_sources = std::mem::take(&mut material.memory_sources);
	let previous_revisions = std::mem::take(&mut material.memory_revisions);
	let mut desired = desired_memory_sources(material, source_sets);
	let mut changed = false;
	for file in &mut material.sources.files {
		if !material.memory_slots.contains(&file.path) {
			continue;
		}
		match desired.remove(&file.path) {
			Some((next, content)) => {
				changed |= !same_source_file(file, &next);
				*file = next;
				material
					.memory_sources
					.insert(file.path.to_path_buf(), content);
			}
			None => {
				if !file.retired {
					file.retired = true;
					changed = true;
				}
				material.memory_sources.remove(&file.path);
			}
		}
	}
	for (path, (file, content)) in desired {
		material.memory_slots.insert(path.to_path_buf());
		material.memory_sources.insert(path, content);
		material.sources.files.push(file);
		changed = true;
	}

	material.memory_revisions = source_sets
		.iter()
		.map(|(srcset, source_set)| (srcset.to_owned(), source_set.revision.to_owned()))
		.collect();
	changed
		|| material.memory_sources != previous_sources
		|| material.memory_revisions != previous_revisions
}

fn sync_memory_source_paths(
	material: &mut SourceCatalogMaterial,
	entries: &BTreeMap<PathBuf, CachedMemorySource>,
	revisions: BTreeMap<String, Option<String>>,
	paths: &[PathBuf],
) -> bool {
	let mut changed = material.memory_revisions != revisions;
	material.memory_revisions = revisions;
	let mut slots = material
		.sources
		.files
		.iter()
		.enumerate()
		.filter(|(_, file)| material.memory_slots.contains(&file.path))
		.map(|(file_idx, file)| (file.path.clone(), file_idx))
		.collect::<BTreeMap<_, _>>();
	let needs_root = paths.iter().any(|path| entries.contains_key(path));
	let root_idx = needs_root.then(|| memory_source_root_index(material));
	for path in paths.iter().filter(|path| is_memory_source_path(path)) {
		match entries.get(path) {
			Some(source) => {
				let root_idx = root_idx.expect("active memory source requires the memory root");
				let next = memory_source_file(material, root_idx, &source.srcset, &source.document);
				match slots.get(path).copied() {
					Some(file_idx) => {
						changed |= !same_source_file(&material.sources.files[file_idx], &next);
						material.sources.files[file_idx] = next;
					}
					None => {
						let file_idx = material.sources.files.len();
						material.sources.files.push(next);
						material.memory_slots.insert(path.clone());
						slots.insert(path.clone(), file_idx);
						changed = true;
					}
				}
				changed |= material.memory_sources.get(path) != Some(&source.document.content);
				material
					.memory_sources
					.insert(path.clone(), source.document.content.clone());
			}
			None => {
				let Some(file_idx) = slots.get(path).copied() else {
					continue;
				};
				if !material.sources.files[file_idx].retired {
					material.sources.files[file_idx].retired = true;
					changed = true;
				}
				changed |= material.memory_sources.remove(path).is_some();
			}
		}
	}
	changed
}

fn desired_memory_sources(
	material: &mut SourceCatalogMaterial,
	source_sets: &BTreeMap<String, MemorySourceSet>,
) -> BTreeMap<PathBuf, (SourceFile, Arc<str>)> {
	let mut desired = BTreeMap::new();
	if source_sets.is_empty() {
		return desired;
	}
	let root_idx = memory_source_root_index(material);
	for (srcset, source_set) in source_sets {
		for document in &source_set.documents {
			let path = memory_source_path(srcset, &document.uri);
			let file = memory_source_file(material, root_idx, srcset, document);
			desired.insert(path, (file, document.content.clone()));
		}
	}
	desired
}

fn memory_source_file(
	material: &SourceCatalogMaterial,
	root_idx: usize,
	srcset: &str,
	document: &MemorySourceDocument,
) -> SourceFile {
	let path = memory_source_path(srcset, &document.uri);
	let uri = PathBuf::from(&document.uri);
	let mut ctx = material.sources.roots[root_idx].ctx.clone();
	ctx.srcset = Some(srcset.to_string());
	SourceFile {
		source: root_idx,
		path,
		rel_path: uri.clone(),
		anchor: uri.clone(),
		lang: document.lang,
		root_moniker: environment::source_root_moniker(document.lang, &uri, &ctx),
		source_group: None,
		srcset: Some(srcset.to_string()),
		retired: false,
	}
}

fn memory_source_root_index(material: &mut SourceCatalogMaterial) -> usize {
	if let Some(index) = material
		.sources
		.roots
		.iter()
		.position(|root| root.input == Path::new(MEMORY_SOURCE_ROOT))
	{
		return index;
	}
	let project = material
		.sources
		.roots
		.iter()
		.find_map(|root| root.ctx.project.clone());
	let index = material.sources.roots.len();
	let path = PathBuf::from(MEMORY_SOURCE_ROOT);
	material.sources.roots.push(SourceRoot {
		input: path.clone(),
		path,
		label: MEMORY_SOURCE_ROOT_LABEL.to_string(),
		ctx: crate::extract::Context {
			project,
			..Default::default()
		},
		source_groups: Default::default(),
	});
	index
}

fn same_source_file(current: &SourceFile, next: &SourceFile) -> bool {
	current.source == next.source
		&& current.path == next.path
		&& current.rel_path == next.rel_path
		&& current.anchor == next.anchor
		&& current.lang == next.lang
		&& current.root_moniker == next.root_moniker
		&& current.source_group == next.source_group
		&& current.srcset == next.srcset
		&& current.retired == next.retired
}

fn canonical_lookup_path(path: &Path) -> PathBuf {
	if let Ok(canonical) = path.canonicalize() {
		return canonical;
	}
	if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
		&& let Ok(parent) = parent.canonicalize()
	{
		return parent.join(name);
	}
	path.to_path_buf()
}

fn new_source_files(
	material: &SourceCatalogMaterial,
	paths: &[PathBuf],
) -> Vec<crate::sources::SourceFile> {
	let mut added: Vec<crate::sources::SourceFile> = Vec::new();
	for path in paths {
		if !path.is_file() || material.normalized_file_index(path).is_some() {
			continue;
		}
		let Some(file) = crate::sources::source_file_for_new_path(&material.sources, path) else {
			continue;
		};
		let duplicate = material.normalized_file_index(&file.path).is_some()
			|| added.iter().any(|existing| existing.path == file.path);
		if !duplicate {
			added.push(file);
		}
	}
	added
}

fn catalog_units(material: &SourceCatalogMaterial) -> Vec<SourceUnit> {
	material
		.sources
		.files
		.iter()
		.enumerate()
		.filter(|(_, file)| !file.retired)
		.map(|(file_idx, file)| {
			SourceUnit::with_language(
				material.identity.source_id(file_idx, &file.rel_path),
				crate::path_util::portable_path(&file.rel_path),
				file.lang.tag(),
			)
		})
		.collect()
}
