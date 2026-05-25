use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::sources;
use crate::workspace::session::{ReferenceId, SourceId, SymbolId};

#[derive(Clone, Default)]
pub struct LocalResourceCache {
	inner: Arc<Mutex<LocalResourceMaterial>>,
}

impl LocalResourceCache {
	pub(super) fn next_generation(&self) -> crate::workspace::session::ResourceGeneration {
		let mut inner = self.inner.lock().expect("local resource cache poisoned");
		let generation = crate::workspace::session::ResourceGeneration::new(inner.next_generation);
		inner.next_generation += 1;
		generation
	}

	pub(super) fn insert_sources(
		&self,
		generation: crate::workspace::session::ResourceGeneration,
		material: SourceCatalogMaterial,
	) {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.sources
			.insert(generation.value(), material);
	}

	pub(super) fn source_material(
		&self,
		generation: crate::workspace::session::ResourceGeneration,
	) -> Option<SourceCatalogMaterial> {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.sources
			.get(&generation.value())
			.cloned()
	}

	pub(super) fn insert_index(
		&self,
		generation: crate::workspace::session::ResourceGeneration,
		material: CodeIndexMaterial,
	) {
		self.inner
			.lock()
			.expect("local resource cache poisoned")
			.indexes
			.insert(generation.value(), material);
	}

	pub(super) fn index_material(
		&self,
		generation: crate::workspace::session::ResourceGeneration,
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
pub(super) struct SourceCatalogMaterial {
	pub(super) sources: sources::SourceSet,
}

#[derive(Clone)]
pub(super) struct CodeIndexMaterial {
	pub(super) source_catalog: SourceCatalogMaterial,
	pub(super) files: Vec<IndexedSourceFile>,
	pub(super) symbols_by_moniker: FxHashMap<Moniker, SymbolId>,
	pub(super) symbol_monikers: FxHashMap<SymbolId, Moniker>,
	pub(super) reference_targets: FxHashMap<ReferenceId, Moniker>,
}

#[derive(Clone)]
pub(super) struct IndexedSourceFile {
	pub(super) source_root: usize,
	pub(super) source_id: SourceId,
	pub(super) path: PathBuf,
	pub(super) rel_path: PathBuf,
	pub(super) anchor: PathBuf,
	pub(super) lang: Lang,
	pub(super) graph: CodeGraph,
	pub(super) source: String,
}

pub(super) fn source_id(file_idx: usize, rel_path: &std::path::Path) -> SourceId {
	SourceId::new(format!("source:{file_idx}:{}", rel_path.display()))
}

pub(super) fn symbol_id(file_idx: usize, def_idx: usize) -> SymbolId {
	SymbolId::new(format!("symbol:{file_idx}:{def_idx}"))
}

pub(super) fn reference_id(file_idx: usize, ref_idx: usize) -> ReferenceId {
	ReferenceId::new(format!("reference:{file_idx}:{ref_idx}"))
}
