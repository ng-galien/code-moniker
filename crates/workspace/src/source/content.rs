use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::environment::{self, SourceFileSet, SourceRoot};
use crate::snapshot::{ReferenceId, SourceId, SymbolId};

use super::identity::LocalIdentityResolver;

#[derive(Clone, Default)]
pub struct LocalResourceCache {
	inner: Arc<Mutex<LocalResourceMaterial>>,
}

impl LocalResourceCache {
	pub fn next_generation(&self) -> crate::snapshot::ResourceGeneration {
		let mut inner = self.inner.lock().expect("local resource cache poisoned");
		let generation = crate::snapshot::ResourceGeneration::new(inner.next_generation);
		inner.next_generation += 1;
		generation
	}

	pub fn insert_sources(
		&self,
		generation: crate::snapshot::ResourceGeneration,
		material: SourceCatalogMaterial,
	) {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.sources
			.insert(generation.value(), material);
	}

	pub fn source_material(
		&self,
		generation: crate::snapshot::ResourceGeneration,
	) -> Option<SourceCatalogMaterial> {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.sources
			.get(&generation.value())
			.cloned()
	}

	pub fn insert_index(
		&self,
		generation: crate::snapshot::ResourceGeneration,
		material: CodeIndexMaterial,
	) {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.indexes
			.insert(generation.value(), material);
	}

	pub fn index_material(
		&self,
		generation: crate::snapshot::ResourceGeneration,
	) -> Option<CodeIndexMaterial> {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.indexes
			.get(&generation.value())
			.cloned()
	}
}

struct LocalResourceMaterial {
	next_generation: u64,
	sources: BTreeMap<u64, SourceCatalogMaterial>,
	indexes: BTreeMap<u64, CodeIndexMaterial>,
}

impl Default for LocalResourceMaterial {
	fn default() -> Self {
		Self {
			next_generation: 1,
			sources: BTreeMap::new(),
			indexes: BTreeMap::new(),
		}
	}
}

#[derive(Clone)]
pub struct SourceCatalogMaterial {
	pub(crate) sources: SourceFileSet,
	pub(crate) identity: LocalIdentityResolver,
}

impl SourceCatalogMaterial {
	pub(crate) fn source_id_for_file(&self, file_idx: usize) -> Option<SourceId> {
		let file = self.sources.files.get(file_idx)?;
		Some(self.identity.source_id(file_idx, &file.rel_path))
	}

	pub fn source_uri_for_path(&self, path: &Path) -> Option<String> {
		self.source_rel_path(path)
			.map(|rel_path| self.identity.source_uri(rel_path))
	}

	#[allow(dead_code)]
	pub(crate) fn resolve_source(&self, path: &Path) -> Option<ResolvedSourceResource> {
		SourceResourceLookup::new(self).resolve(path)
	}

	fn source_rel_path(&self, path: &Path) -> Option<&Path> {
		let normalized = normalize_path(path);
		self.sources
			.files
			.iter()
			.find(|file| {
				normalize_path(&file.path) == normalized
					|| normalize_path(&file.rel_path) == normalized
					|| normalize_path(&file.anchor) == normalized
			})
			.map(|file| file.rel_path.as_path())
	}

	#[allow(dead_code)]
	fn root_for_path(&self, path: &Path) -> Option<(usize, &SourceRoot)> {
		self.sources
			.roots
			.iter()
			.enumerate()
			.filter_map(|(root_idx, root)| {
				let absolute = absolute_path_against_root(&root.path, path);
				let root_path = normalize_path(&root.path);
				normalize_path(&absolute)
					.starts_with(&root_path)
					.then_some((root_idx, root, root_path.components().count()))
			})
			.max_by_key(|(_, _, depth)| *depth)
			.map(|(root_idx, root, _)| (root_idx, root))
	}
}

#[allow(dead_code)]
struct SourceResourceLookup<'a> {
	material: &'a SourceCatalogMaterial,
}

impl<'a> SourceResourceLookup<'a> {
	fn new(material: &'a SourceCatalogMaterial) -> Self {
		Self { material }
	}

	fn resolve(&self, path: &Path) -> Option<ResolvedSourceResource> {
		self.indexed(path).or_else(|| self.lazy(path))
	}

	fn indexed(&self, path: &Path) -> Option<ResolvedSourceResource> {
		let normalized = normalize_path(path);
		let (file_idx, file) =
			self.material
				.sources
				.files
				.iter()
				.enumerate()
				.find(|(_, file)| {
					normalize_path(&file.path) == normalized
						|| normalize_path(&file.rel_path) == normalized
				})?;
		Some(ResolvedSourceResource {
			source_root: file.source,
			source_id: self.material.identity.source_id(file_idx, &file.rel_path),
			source_uri: self.material.identity.source_uri(&file.rel_path),
			path: file.path.clone(),
			rel_path: file.rel_path.clone(),
			anchor: file.anchor.clone(),
			lang: file.lang,
			eager_index: Some(file_idx),
		})
	}

	fn lazy(&self, path: &Path) -> Option<ResolvedSourceResource> {
		let (source_root, root) = self.material.root_for_path(path)?;
		let abs_path = absolute_path_against_root(&root.path, path);
		if !abs_path.is_file() {
			return None;
		}
		let lang = environment::language_for_path(&abs_path).ok()?;
		let rel = abs_path.strip_prefix(&root.path).ok()?.to_path_buf();
		let rel_path = self.rel_path(root, &rel);
		Some(ResolvedSourceResource {
			source_root,
			source_id: SourceId::new(format!("source:lazy:{}", rel_path.display())),
			source_uri: self.material.identity.source_uri(&rel_path),
			path: abs_path,
			rel_path,
			anchor: rel,
			lang,
			eager_index: None,
		})
	}

	fn rel_path(&self, root: &SourceRoot, rel: &Path) -> PathBuf {
		if self.material.sources.multi {
			PathBuf::from(&root.label).join(rel)
		} else {
			rel.to_path_buf()
		}
	}
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct ResolvedSourceResource {
	pub(crate) source_root: usize,
	pub(crate) source_id: SourceId,
	pub(crate) source_uri: String,
	pub(crate) path: PathBuf,
	pub(crate) rel_path: PathBuf,
	pub(crate) anchor: PathBuf,
	pub(crate) lang: Lang,
	pub(crate) eager_index: Option<usize>,
}

#[derive(Clone)]
pub struct CodeIndexMaterial {
	pub source_catalog: SourceCatalogMaterial,
	pub files: Vec<IndexedSourceFile>,
	pub identity: LocalIdentityResolver,
	pub symbols_by_moniker: FxHashMap<Moniker, SymbolId>,
	pub symbol_monikers: FxHashMap<SymbolId, Moniker>,
	pub reference_targets: FxHashMap<ReferenceId, Moniker>,
}

#[derive(Clone)]
pub struct IndexedSourceFile {
	pub source_root: usize,
	pub source_id: SourceId,
	pub source_uri: String,
	pub identity: LocalIdentityResolver,
	pub path: PathBuf,
	pub rel_path: PathBuf,
	pub anchor: PathBuf,
	pub lang: Lang,
	pub graph: CodeGraph,
	pub source: String,
}

fn normalize_path(path: &Path) -> PathBuf {
	let mut out = PathBuf::new();
	for component in path.components() {
		match component {
			std::path::Component::CurDir => {}
			std::path::Component::ParentDir => {
				out.pop();
			}
			_ => out.push(component.as_os_str()),
		}
	}
	out
}

#[allow(dead_code)]
fn absolute_path_against_root(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() {
		normalize_path(path)
	} else {
		normalize_path(&root.join(path))
	}
}
