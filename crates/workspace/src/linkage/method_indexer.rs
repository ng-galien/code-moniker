use std::collections::BTreeSet;

use crate::linkage::semantic::MethodTable;
use crate::snapshot::SourceId;
use crate::source::CodeIndexMaterial;

/// Builds the [`MethodTable`] and keeps it current as files are edited: it
/// patches the methods of changed files when the file set is stable, and
/// rebuilds from scratch when files are added or removed (file indexes are
/// positional, so an add/remove shifts the ids encoded in every symbol).
pub(super) struct MethodIndexer {
	methods: MethodTable,
	file_source_ids: Vec<SourceId>,
}

impl MethodIndexer {
	pub(super) fn new(material: &CodeIndexMaterial) -> Self {
		Self {
			methods: MethodTable::build(material),
			file_source_ids: file_source_ids(material),
		}
	}

	pub(super) fn reindex(
		&mut self,
		material: &CodeIndexMaterial,
		changed_file_indexes: &BTreeSet<usize>,
	) -> &MethodTable {
		let next_source_ids = file_source_ids(material);
		if self.file_source_ids == next_source_ids {
			self.methods.refresh_files(material, changed_file_indexes);
		} else {
			self.methods = MethodTable::build(material);
		}
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
