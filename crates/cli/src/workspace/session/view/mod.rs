mod changes;
mod diagnostics;
mod linkage;
mod model;
mod references;
mod search;
mod sources;
mod symbols;

use super::model::WorkspaceSnapshot;

pub use changes::ChangeView;
pub use diagnostics::DiagnosticsView;
pub use linkage::LinkageView;
pub use model::{
	ChangeDetail, ChangeSummary, DiagnosticsSummary, ReferenceDirection, ReferenceSet,
	ReferenceSetSummary, ReferenceSummary, SearchHit, SourceSummary, SymbolDetail,
	SymbolReferences, SymbolSummary, UnresolvedLinkageReport,
};
pub use references::ReferenceView;
pub use search::SearchView;
pub use sources::SourceView;
pub use symbols::SymbolView;

pub struct WorkspaceView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> WorkspaceView<'a> {
	pub fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn sources(&self) -> SourceView<'a> {
		SourceView::new(self.snapshot)
	}

	pub fn symbols(&self) -> SymbolView<'a> {
		SymbolView::new(self.snapshot)
	}

	pub fn references(&self) -> ReferenceView<'a> {
		ReferenceView::new(self.snapshot)
	}

	pub fn search(&self) -> SearchView<'a> {
		SearchView::new(self.snapshot)
	}

	pub fn changes(&self) -> ChangeView<'a> {
		ChangeView::new(self.snapshot)
	}

	pub fn linkage(&self) -> LinkageView<'a> {
		LinkageView::new(self.snapshot)
	}

	pub fn diagnostics(&self) -> DiagnosticsView<'a> {
		DiagnosticsView::new(self.snapshot)
	}
}
