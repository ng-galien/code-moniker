use super::model::{SymbolDetail, SymbolSummary};
use crate::workspace::session::model::{ChangeStatus, SymbolId, SymbolRecord, WorkspaceSnapshot};

pub struct SymbolView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> SymbolView<'a> {
	pub(super) fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn all(&self) -> Vec<SymbolSummary> {
		let mut symbols = self
			.snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.navigable)
			.map(|symbol| self.summary(symbol))
			.collect::<Vec<_>>();
		symbols.sort_by(|left, right| {
			left.source
				.as_str()
				.cmp(right.source.as_str())
				.then_with(|| left.identity.cmp(&right.identity))
		});
		symbols
	}

	pub fn roots_for_source(
		&self,
		source: &crate::workspace::session::SourceId,
	) -> Vec<SymbolSummary> {
		self.snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| {
				symbol.navigable && &symbol.source == source && symbol.parent.is_none()
			})
			.map(|symbol| self.summary(symbol))
			.collect()
	}

	pub fn children(&self, parent: &SymbolId) -> Vec<SymbolSummary> {
		self.snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.navigable && symbol.parent.as_ref() == Some(parent))
			.map(|symbol| self.summary(symbol))
			.collect()
	}

	pub fn detail(&self, symbol: &SymbolId) -> Option<SymbolDetail> {
		let record = self.find(symbol)?;
		Some(SymbolDetail {
			symbol: self.summary(record),
			children: self.children(symbol),
		})
	}

	pub(super) fn find(&self, id: &SymbolId) -> Option<&SymbolRecord> {
		self.snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| &symbol.id == id)
	}

	pub(super) fn summary(&self, symbol: &SymbolRecord) -> SymbolSummary {
		SymbolSummary {
			id: symbol.id.clone(),
			source: symbol.source.clone(),
			identity: symbol.identity.clone(),
			name: symbol.name.clone(),
			kind: symbol.kind.clone(),
			line_range: symbol.line_range,
			child_count: self
				.snapshot
				.index
				.symbols
				.iter()
				.filter(|child| child.navigable && child.parent.as_ref() == Some(&symbol.id))
				.count(),
			change: self.change_status_for_symbol(&symbol.id),
		}
	}

	fn change_status_for_symbol(&self, symbol: &SymbolId) -> Option<ChangeStatus> {
		self.snapshot
			.changes
			.changes
			.iter()
			.find(|change| change.symbol.as_ref() == Some(symbol))
			.map(|change| change.status)
	}
}
