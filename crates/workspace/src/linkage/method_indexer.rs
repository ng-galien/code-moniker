use std::collections::BTreeSet;

use crate::linkage::candidate::CandidateCatalog;
use crate::linkage::semantic::MethodTable;
use crate::snapshot::SourceId;
use crate::source::CodeIndexMaterial;

/// Builds the [`MethodTable`] and keeps it aligned with the generation-local
/// symbol ordinal catalog.
pub(super) struct MethodIndexer {
	methods: MethodTable,
	file_source_ids: Vec<SourceId>,
}

impl MethodIndexer {
	pub(super) fn new(material: &CodeIndexMaterial, candidates: &CandidateCatalog<'_>) -> Self {
		Self {
			methods: MethodTable::build(material, candidates),
			file_source_ids: file_source_ids(material),
		}
	}

	pub(super) fn reindex(
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

	pub(super) fn methods(&self) -> &MethodTable {
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
