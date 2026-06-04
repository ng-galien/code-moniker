use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::environment::{self, SourceFileSet, SourceRoot};
use crate::path_util::lexical_path;
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
		let mut inner = self.inner.lock().expect("local resource cache poisoned");
		inner.sources.clear();
		inner.sources.insert(generation.value(), material);
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
		let mut inner = self.inner.lock().expect("local resource cache poisoned");
		inner.indexes.clear();
		inner.indexes.insert(generation.value(), Arc::new(material));
	}

	pub fn index_material(
		&self,
		generation: crate::snapshot::ResourceGeneration,
	) -> Option<Arc<CodeIndexMaterial>> {
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
	indexes: BTreeMap<u64, Arc<CodeIndexMaterial>>,
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
		self.normalized_file_index(path)
			.map(|file_idx| self.sources.files[file_idx].rel_path.as_path())
	}

	fn normalized_file_index(&self, path: &Path) -> Option<usize> {
		let normalized = normalize_path(path);
		self.sources.files.iter().position(|file| {
			normalize_path(&file.path) == normalized
				|| normalize_path(&file.rel_path) == normalized
				|| normalize_path(&file.anchor) == normalized
		})
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
		let file_idx = self.match_indexed_file(path)?;
		let file = self.material.sources.files.get(file_idx)?;
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

	fn match_indexed_file(&self, path: &Path) -> Option<usize> {
		self.material
			.sources
			.files
			.iter()
			.enumerate()
			.filter(|(_, file)| path.ends_with(&file.rel_path))
			.max_by_key(|(_, file)| file.rel_path.components().count())
			.map(|(file_idx, _)| file_idx)
			.or_else(|| self.material.normalized_file_index(path))
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
}

impl CodeIndexMaterial {
	pub fn symbol_moniker(&self, symbol: &SymbolId) -> Option<&Moniker> {
		let (file_idx, def_idx) = self.identity.symbol_location(symbol)?;
		let graph = &self.files.get(file_idx)?.graph;
		(def_idx < graph.def_count()).then(|| &graph.def_at(def_idx).moniker)
	}

	pub fn symbol_source(&self, symbol: &SymbolId) -> Option<SourceId> {
		let (file_idx, def_idx) = self.identity.symbol_location(symbol)?;
		let file = self.files.get(file_idx)?;
		(def_idx < file.graph.def_count()).then(|| file.source_id.clone())
	}

	pub fn symbol_exists(&self, symbol: &SymbolId) -> bool {
		self.symbol_moniker(symbol).is_some()
	}

	pub fn reference_target(&self, reference: &ReferenceId) -> Option<&Moniker> {
		let (file_idx, ref_idx) = self.identity.reference_location(reference)?;
		let graph = &self.files.get(file_idx)?.graph;
		(ref_idx < graph.ref_count()).then(|| &graph.ref_at(ref_idx).target)
	}

	pub fn symbols(&self) -> impl Iterator<Item = (SymbolId, &Moniker)> + '_ {
		self.files.iter().enumerate().flat_map(|(file_idx, file)| {
			file.graph.defs().enumerate().map(move |(def_idx, def)| {
				(file.identity.symbol_id(file_idx, def_idx), &def.moniker)
			})
		})
	}
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
	lexical_path(path)
}

#[allow(dead_code)]
fn absolute_path_against_root(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() {
		normalize_path(path)
	} else {
		normalize_path(&root.join(path))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use code_moniker_core::core::moniker::MonikerBuilder;
	use code_moniker_core::lang::Lang;

	#[test]
	fn symbol_moniker_returns_none_for_out_of_range_symbol_id() {
		let (material, root, _) = material_with_one_reference();

		assert_eq!(
			material.symbol_moniker(&SymbolId::new("symbol:0:0")),
			Some(&root)
		);
		assert!(
			material
				.symbol_moniker(&SymbolId::new("symbol:0:999999"))
				.is_none()
		);
	}

	#[test]
	fn reference_target_returns_none_for_out_of_range_reference_id() {
		let (material, _, target) = material_with_one_reference();

		assert_eq!(
			material.reference_target(&ReferenceId::new("reference:0:0")),
			Some(&target)
		);
		assert!(
			material
				.reference_target(&ReferenceId::new("reference:0:999999"))
				.is_none()
		);
	}

	fn material_with_one_reference() -> (CodeIndexMaterial, Moniker, Moniker) {
		let identity = LocalIdentityResolver::default();
		let root = MonikerBuilder::new()
			.project(b"app")
			.segment(b"module", b"main")
			.build();
		let target = MonikerBuilder::new()
			.project(b"app")
			.segment(b"module", b"other")
			.build();
		let mut graph = CodeGraph::new(root.clone(), b"module");
		graph
			.add_ref(&root, target.clone(), b"calls", None)
			.expect("test graph ref must be valid");
		let rel_path = PathBuf::from("main.rs");
		let file = IndexedSourceFile {
			source_root: 0,
			source_id: identity.source_id(0, &rel_path),
			source_uri: identity.source_uri(&rel_path),
			identity: identity.clone(),
			path: rel_path.clone(),
			rel_path: rel_path.clone(),
			anchor: rel_path,
			lang: Lang::Rs,
			graph,
			source: String::new(),
		};
		let material = CodeIndexMaterial {
			source_catalog: SourceCatalogMaterial {
				sources: SourceFileSet {
					roots: Vec::new(),
					files: Vec::new(),
					multi: false,
				},
				identity: identity.clone(),
			},
			files: vec![file],
			identity,
			symbols_by_moniker: FxHashMap::default(),
		};
		(material, root, target)
	}
}
