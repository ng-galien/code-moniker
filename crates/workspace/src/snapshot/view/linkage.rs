use std::collections::BTreeMap;

use super::model::UnresolvedLinkageReport;
use crate::snapshot::model::{ReferenceId, SourceId, UnresolvedReason, WorkspaceSnapshot};

pub struct LinkageView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> LinkageView<'a> {
	pub(super) fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn unresolved_report(&self) -> UnresolvedLinkageReport {
		let mut sources = BTreeMap::<SourceId, usize>::new();
		let mut reasons = BTreeMap::<UnresolvedReason, usize>::new();
		for unresolved in self
			.snapshot
			.linkage
			.unresolved
			.iter()
			.chain(&self.snapshot.linkage.manifest_blocked)
		{
			if let Some(source) = self.reference_source(&unresolved.reference) {
				*sources.entry(source).or_default() += 1;
			}
			*reasons.entry(unresolved.reason).or_default() += 1;
		}
		UnresolvedLinkageReport {
			unresolved_refs: self.snapshot.linkage.unresolved_refs,
			sources,
			reasons,
		}
	}

	fn reference_source(&self, reference: &ReferenceId) -> Option<SourceId> {
		self.snapshot
			.index
			.references
			.iter()
			.find(|candidate| &candidate.id == reference)
			.map(|candidate| candidate.source)
	}
}
