use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::environment::{self, SourceFileSet, SourceRoot};
use crate::path_util::lexical_path;
use crate::snapshot::{ReferenceId, SourceId, SymbolId};

use super::identity::LocalIdentityResolver;

pub const MEMORY_SOURCE_ROOT: &str = "memory";
pub const MEMORY_SOURCE_ROOT_LABEL: &str = "memory";
const MEMORY_SOURCE_PATH_ROOT: &str = ".code-moniker-memory";

pub fn is_memory_source_path(path: &Path) -> bool {
	path.starts_with(MEMORY_SOURCE_PATH_ROOT)
}

#[derive(Clone, Default)]
pub struct LocalResourceCache {
	inner: Arc<Mutex<LocalResourceMaterial>>,
}

impl LocalResourceCache {
	pub fn next_generation(&self) -> crate::snapshot::ResourceGeneration {
		let mut inner = self.lock_material();
		let generation = crate::snapshot::ResourceGeneration::new(inner.next_generation);
		inner.next_generation += 1;
		generation
	}

	fn lock_material(&self) -> std::sync::MutexGuard<'_, LocalResourceMaterial> {
		self.inner
			.lock()
			.unwrap_or_else(|poisoned| poisoned.into_inner())
	}

	pub fn insert_sources(
		&self,
		generation: crate::snapshot::ResourceGeneration,
		material: SourceCatalogMaterial,
	) {
		let mut inner = self.lock_material();
		inner.sources.clear();
		inner.sources.insert(generation.value(), material);
	}

	pub fn source_material(
		&self,
		generation: crate::snapshot::ResourceGeneration,
	) -> Option<SourceCatalogMaterial> {
		self.lock_material()
			.sources
			.get(&generation.value())
			.cloned()
	}

	pub fn insert_index(
		&self,
		generation: crate::snapshot::ResourceGeneration,
		material: CodeIndexMaterial,
	) {
		let mut inner = self.lock_material();
		inner.indexes.clear();
		inner.index_diffs.clear();
		inner.indexes.insert(generation.value(), Arc::new(material));
	}

	pub fn insert_index_diff(
		&self,
		generation: crate::snapshot::ResourceGeneration,
		previous_generation: crate::snapshot::ResourceGeneration,
		diff: crate::code::CodeIndexGraphDiff,
	) {
		self.lock_material()
			.index_diffs
			.insert(generation.value(), (previous_generation, Arc::new(diff)));
	}

	pub fn index_diff(
		&self,
		generation: crate::snapshot::ResourceGeneration,
	) -> Option<(
		crate::snapshot::ResourceGeneration,
		Arc<crate::code::CodeIndexGraphDiff>,
	)> {
		self.lock_material()
			.index_diffs
			.get(&generation.value())
			.cloned()
	}

	pub fn index_material(
		&self,
		generation: crate::snapshot::ResourceGeneration,
	) -> Option<Arc<CodeIndexMaterial>> {
		self.lock_material()
			.indexes
			.get(&generation.value())
			.cloned()
	}

	pub fn replace_memory_source_set(&self, source_set: MemorySourceSet) -> MemorySourceSetUpdate {
		let mut inner = self.lock_material();
		if inner.memory_source_sets.get(&source_set.srcset) == Some(&source_set) {
			return MemorySourceSetUpdate {
				srcset: source_set.srcset,
				..Default::default()
			};
		}
		let srcset = source_set.srcset.clone();
		let next_paths = memory_source_paths(&source_set);
		let previous = inner.memory_source_sets.insert(srcset.clone(), source_set);
		let mut paths = previous
			.as_ref()
			.into_iter()
			.flat_map(memory_source_paths)
			.collect::<BTreeSet<_>>();
		paths.extend(next_paths);
		MemorySourceSetUpdate {
			changed: true,
			paths: paths.into_iter().collect(),
			srcset,
			previous,
		}
	}

	pub fn remove_memory_source_set(&self, srcset: &str) -> MemorySourceSetUpdate {
		let mut inner = self.lock_material();
		let Some(previous) = inner.memory_source_sets.remove(srcset) else {
			return MemorySourceSetUpdate {
				srcset: srcset.to_string(),
				..Default::default()
			};
		};
		MemorySourceSetUpdate {
			changed: true,
			paths: memory_source_paths(&previous),
			srcset: srcset.to_string(),
			previous: Some(previous),
		}
	}

	pub fn restore_memory_source_set(&self, srcset: String, previous: Option<MemorySourceSet>) {
		let mut inner = self.lock_material();
		match previous {
			Some(previous) => {
				inner.memory_source_sets.insert(srcset, previous);
			}
			None => {
				inner.memory_source_sets.remove(&srcset);
			}
		}
	}

	pub fn memory_source_usage_after_replacing(
		&self,
		source_set: &MemorySourceSet,
	) -> (usize, usize, usize) {
		let inner = self.lock_material();
		let mut source_sets = 0usize;
		let mut documents = 0usize;
		let mut bytes = 0usize;
		for (srcset, active) in &inner.memory_source_sets {
			if srcset == &source_set.srcset {
				continue;
			}
			source_sets = source_sets.saturating_add(1);
			documents = documents.saturating_add(active.documents.len());
			bytes = bytes.saturating_add(active.size_bytes());
		}
		(
			source_sets.saturating_add(1),
			documents.saturating_add(source_set.documents.len()),
			bytes.saturating_add(source_set.size_bytes()),
		)
	}

	pub(crate) fn memory_source_sets(&self) -> BTreeMap<String, MemorySourceSet> {
		self.lock_material().memory_source_sets.clone()
	}
}

struct LocalResourceMaterial {
	next_generation: u64,
	sources: BTreeMap<u64, SourceCatalogMaterial>,
	indexes: BTreeMap<u64, Arc<CodeIndexMaterial>>,
	memory_source_sets: BTreeMap<String, MemorySourceSet>,
	index_diffs: BTreeMap<
		u64,
		(
			crate::snapshot::ResourceGeneration,
			Arc<crate::code::CodeIndexGraphDiff>,
		),
	>,
}

impl Default for LocalResourceMaterial {
	fn default() -> Self {
		Self {
			next_generation: 1,
			sources: BTreeMap::new(),
			indexes: BTreeMap::new(),
			memory_source_sets: BTreeMap::new(),
			index_diffs: BTreeMap::new(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemorySourceSet {
	pub srcset: String,
	pub revision: Option<String>,
	pub documents: Vec<MemorySourceDocument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemorySourceDocument {
	pub uri: String,
	pub lang: Lang,
	pub content: String,
}

impl MemorySourceSet {
	pub fn size_bytes(&self) -> usize {
		self.srcset
			.len()
			.saturating_add(self.revision.as_ref().map_or(0, String::len))
			.saturating_add(self.documents.iter().fold(0usize, |total, document| {
				total
					.saturating_add(document.uri.len())
					.saturating_add(document.content.len())
					.saturating_add(document.lang.tag().len())
			}))
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MemorySourceSetUpdate {
	pub changed: bool,
	pub paths: Vec<PathBuf>,
	pub srcset: String,
	pub previous: Option<MemorySourceSet>,
}

pub(crate) fn memory_source_path(srcset: &str, uri: &str) -> PathBuf {
	PathBuf::from(MEMORY_SOURCE_PATH_ROOT)
		.join(srcset)
		.join(hex_path_component(uri.as_bytes()))
}

fn hex_path_component(value: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";
	let mut encoded = String::with_capacity(value.len() * 2);
	for byte in value {
		encoded.push(HEX[(byte >> 4) as usize] as char);
		encoded.push(HEX[(byte & 0x0f) as usize] as char);
	}
	encoded
}

fn memory_source_paths(source_set: &MemorySourceSet) -> Vec<PathBuf> {
	source_set
		.documents
		.iter()
		.map(|document| memory_source_path(&source_set.srcset, &document.uri))
		.collect()
}

#[derive(Clone)]
pub struct SourceCatalogMaterial {
	pub(crate) sources: SourceFileSet,
	pub(crate) identity: LocalIdentityResolver,
	pub(crate) memory_sources: BTreeMap<PathBuf, String>,
	pub(crate) memory_slots: BTreeSet<PathBuf>,
	pub(crate) memory_revisions: BTreeMap<String, Option<String>>,
}

impl SourceCatalogMaterial {
	pub(crate) fn source_id_for_file(&self, file_idx: usize) -> Option<SourceId> {
		let file = self.sources.files.get(file_idx)?;
		Some(self.identity.source_id(file_idx, &file.rel_path))
	}

	pub fn source_uri_for_path(&self, path: &Path) -> Option<String> {
		let file_idx = self.normalized_file_index(path)?;
		let file = self.sources.files.get(file_idx)?;
		let rel_path = file.rel_path.as_path();
		Some(
			match self
				.is_memory_slot(&file.path)
				.then_some(file.srcset.as_deref())
				.flatten()
			{
				Some(srcset) => {
					let rel_path = crate::path_util::portable_path(rel_path);
					let moniker = MonikerBuilder::new()
						.project(b".")
						.segment(b"srcset", srcset.as_bytes())
						.segment(b"file", rel_path.as_bytes())
						.build();
					self.identity.moniker_uri(&moniker)
				}
				None => self.identity.source_uri(rel_path),
			},
		)
	}

	#[allow(dead_code)]
	pub(crate) fn resolve_source(&self, path: &Path) -> Option<ResolvedSourceResource> {
		SourceResourceLookup::new(self).resolve(path)
	}

	pub(crate) fn normalized_file_index(&self, path: &Path) -> Option<usize> {
		let normalized = normalize_path(path);
		self.sources.files.iter().position(|file| {
			normalize_path(&file.path) == normalized
				|| normalize_path(&file.rel_path) == normalized
				|| normalize_path(&file.anchor) == normalized
		})
	}

	pub(crate) fn memory_source(&self, path: &Path) -> Option<&str> {
		self.memory_sources.get(path).map(String::as_str)
	}

	pub(crate) fn is_memory_slot(&self, path: &Path) -> bool {
		self.memory_slots.contains(path)
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
			source_id: SourceId::at(u32::MAX as usize),
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
	pub files: Vec<Arc<IndexedSourceFile>>,
	pub identity: LocalIdentityResolver,
	pub symbols_by_moniker: FxHashMap<Moniker, SymbolId>,
}

impl CodeIndexMaterial {
	pub fn source_set(&self) -> &SourceFileSet {
		&self.source_catalog.sources
	}

	pub fn symbol_moniker(&self, symbol: &SymbolId) -> Option<&Moniker> {
		let (file_idx, def_idx) = self.identity.symbol_location(symbol)?;
		let graph = &self.files.get(file_idx)?.graph;
		(def_idx < graph.def_count()).then(|| &graph.def_at(def_idx).moniker)
	}

	pub fn symbol_source(&self, symbol: &SymbolId) -> Option<SourceId> {
		let (file_idx, def_idx) = self.identity.symbol_location(symbol)?;
		let file = self.files.get(file_idx)?;
		(def_idx < file.graph.def_count()).then(|| file.source_id)
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
	pub extraction_cache: &'static str,
	pub extraction_duration: Duration,
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

		assert_eq!(material.symbol_moniker(&SymbolId::at(0, 0)), Some(&root));
		assert!(material.symbol_moniker(&SymbolId::at(0, 999999)).is_none());
	}

	#[test]
	fn reference_target_returns_none_for_out_of_range_reference_id() {
		let (material, _, target) = material_with_one_reference();

		assert_eq!(
			material.reference_target(&ReferenceId::at(0, 0)),
			Some(&target)
		);
		assert!(
			material
				.reference_target(&ReferenceId::at(0, 999999))
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
			extraction_cache: "provided",
			extraction_duration: Duration::ZERO,
		};
		let material = CodeIndexMaterial {
			source_catalog: SourceCatalogMaterial {
				sources: SourceFileSet {
					roots: Vec::new(),
					files: Vec::new(),
					multi: false,
				},
				identity: identity.clone(),
				memory_sources: BTreeMap::new(),
				memory_slots: BTreeSet::new(),
				memory_revisions: BTreeMap::new(),
			},
			files: vec![Arc::new(file)],
			identity,
			symbols_by_moniker: FxHashMap::default(),
		};
		(material, root, target)
	}
}
