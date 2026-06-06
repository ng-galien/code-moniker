use std::collections::BTreeSet;

use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::resolve::MethodTable;
use crate::snapshot::SourceId;
use crate::source::CodeIndexMaterial;

/// Builds the [`MethodTable`] and keeps it aligned with the generation-local
/// symbol ordinal catalog.
pub(in crate::linkage) struct MethodIndexer {
	methods: MethodTable,
	file_source_ids: Vec<SourceId>,
}

impl MethodIndexer {
	pub(in crate::linkage) fn new(
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
	) -> Self {
		Self {
			methods: MethodTable::build(material, candidates),
			file_source_ids: file_source_ids(material),
		}
	}

	pub(in crate::linkage) fn reindex(
		&mut self,
		material: &CodeIndexMaterial,
		candidates: &CandidateCatalog<'_>,
		_changed_file_indexes: &BTreeSet<usize>,
	) -> &MethodTable {
		let next_source_ids = file_source_ids(material);
		self.methods = MethodTable::build(material, candidates);
		self.file_source_ids = next_source_ids;
		&self.methods
	}

	pub(in crate::linkage) fn methods(&self) -> &MethodTable {
		&self.methods
	}
}

fn file_source_ids(material: &CodeIndexMaterial) -> Vec<SourceId> {
	material
		.files
		.iter()
		.map(|file| file.source_id.clone())
		.collect()
}
