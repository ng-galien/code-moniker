use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::sources;
use crate::workspace::snapshot::{ReferenceId, SourceId, SymbolId};

use super::identity::LocalIdentityResolver;

#[derive(Clone, Default)]
pub struct LocalResourceCache {
	inner: Arc<Mutex<LocalResourceMaterial>>,
}

impl LocalResourceCache {
	pub(crate) fn next_generation(&self) -> crate::workspace::snapshot::ResourceGeneration {
		let mut inner = self.inner.lock().expect("local resource cache poisoned");
		let generation = crate::workspace::snapshot::ResourceGeneration::new(inner.next_generation);
		inner.next_generation += 1;
		generation
	}

	pub(crate) fn insert_sources(
		&self,
		generation: crate::workspace::snapshot::ResourceGeneration,
		material: SourceCatalogMaterial,
	) {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.sources
			.insert(generation.value(), material);
	}

	pub(crate) fn source_material(
		&self,
		generation: crate::workspace::snapshot::ResourceGeneration,
	) -> Option<SourceCatalogMaterial> {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.sources
			.get(&generation.value())
			.cloned()
	}

	pub(crate) fn insert_index(
		&self,
		generation: crate::workspace::snapshot::ResourceGeneration,
		material: CodeIndexMaterial,
	) {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.indexes
			.insert(generation.value(), material);
	}

	pub(crate) fn index_material(
		&self,
		generation: crate::workspace::snapshot::ResourceGeneration,
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
pub(crate) struct SourceCatalogMaterial {
	pub(crate) sources: sources::SourceSet,
	pub(crate) identity: LocalIdentityResolver,
}

impl SourceCatalogMaterial {
	pub(crate) fn source_id_for_file(&self, file_idx: usize) -> Option<SourceId> {
		let file = self.sources.files.get(file_idx)?;
		Some(self.identity.source_id(file_idx, &file.rel_path))
	}

	pub(crate) fn source_uri_for_path(&self, path: &Path) -> Option<String> {
		self.source_rel_path(path)
			.map(|rel_path| self.identity.source_uri(rel_path))
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
}

#[derive(Clone)]
pub(crate) struct CodeIndexMaterial {
	pub(crate) source_catalog: SourceCatalogMaterial,
	pub(crate) files: Vec<IndexedSourceFile>,
	pub(crate) identity: LocalIdentityResolver,
	pub(crate) symbols_by_moniker: FxHashMap<Moniker, SymbolId>,
	pub(crate) symbol_monikers: FxHashMap<SymbolId, Moniker>,
	pub(crate) reference_targets: FxHashMap<ReferenceId, Moniker>,
}

#[derive(Clone)]
pub(crate) struct IndexedSourceFile {
	pub(crate) source_root: usize,
	pub(crate) source_id: SourceId,
	pub(crate) source_uri: String,
	pub(crate) identity: LocalIdentityResolver,
	pub(crate) path: PathBuf,
	pub(crate) rel_path: PathBuf,
	pub(crate) anchor: PathBuf,
	pub(crate) lang: Lang,
	pub(crate) graph: CodeGraph,
	pub(crate) source: String,
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
