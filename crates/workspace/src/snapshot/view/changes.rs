// code-moniker: ignore-file[smell-clone-reflex]
// Snapshot views are owned read-model projections from borrowed workspace records.
use super::model::{ChangeDetail, ChangeSummary, ReferenceDirection, ReferenceSet};
use super::references::ReferenceView;
use crate::snapshot::model::{ChangeId, ChangeRecord, SymbolId, WorkspaceSnapshot};

pub struct ChangeView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> ChangeView<'a> {
	pub(super) fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn summaries(&self) -> Vec<ChangeSummary> {
		self.snapshot
			.changes
			.changes
			.iter()
			.map(|change| self.summary(change))
			.collect()
	}

	pub fn detail(&self, change: &ChangeId) -> Option<ChangeDetail> {
		let record = self
			.snapshot
			.changes
			.changes
			.iter()
			.find(|candidate| &candidate.id == change)?;
		let blast_radius = record
			.symbol
			.as_ref()
			.map(|symbol| self.blast_radius(symbol))
			.unwrap_or_else(empty_reference_set);
		Some(ChangeDetail {
			summary: self.summary(record),
			blast_radius,
		})
	}

	fn summary(&self, change: &ChangeRecord) -> ChangeSummary {
		ChangeSummary {
			id: change.id.clone(),
			status: change.status,
			source: change.source.clone(),
			symbol: change.symbol.clone(),
			identity: change.identity.clone(),
			name: change.name.clone(),
			kind: change.kind.clone(),
			line_range: change.line_range,
			hunk_count: change.hunk_count,
			usage_count: change
				.symbol
				.as_ref()
				.map(|symbol| ReferenceView::new(self.snapshot).incoming_ids(symbol).len())
				.unwrap_or(0),
		}
	}

	fn blast_radius(&self, symbol: &SymbolId) -> ReferenceSet {
		let references = ReferenceView::new(self.snapshot);
		references.reference_set(
			&references.incoming_ids(symbol),
			ReferenceDirection::Incoming,
		)
	}
}

fn empty_reference_set() -> ReferenceSet {
	ReferenceSet {
		summary: super::model::ReferenceSetSummary {
			refs: 0,
			files: 0,
			contexts: 0,
		},
		groups: Vec::new(),
	}
}
