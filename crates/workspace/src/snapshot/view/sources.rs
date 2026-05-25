use super::model::SourceSummary;
use crate::snapshot::model::{SourceId, WorkspaceSnapshot};

pub struct SourceView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> SourceView<'a> {
	pub(super) fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn all(&self) -> Vec<SourceSummary> {
		self.snapshot
			.catalog
			.sources
			.iter()
			.map(|source| SourceSummary {
				id: source.id.clone(),
				display_name: source.display_name.clone(),
				language: source.language.clone(),
				change_count: self.change_count_for_source(&source.id),
			})
			.collect()
	}

	pub fn find(&self, source: &SourceId) -> Option<SourceSummary> {
		self.all()
			.into_iter()
			.find(|candidate| &candidate.id == source)
	}

	fn change_count_for_source(&self, source: &SourceId) -> usize {
		self.snapshot
			.changes
			.changes
			.iter()
			.filter(|change| change.source.as_ref() == Some(source))
			.count()
	}
}
