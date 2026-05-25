use super::model::DiagnosticsSummary;
use crate::workspace::session::model::WorkspaceSnapshot;

pub struct DiagnosticsView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> DiagnosticsView<'a> {
	pub(super) fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn summary(&self) -> DiagnosticsSummary {
		DiagnosticsSummary {
			errors: self.snapshot.diagnostics.errors,
			warnings: self.snapshot.diagnostics.warnings,
			total: self.snapshot.diagnostics.diagnostics.len(),
		}
	}
}
