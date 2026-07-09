// code-moniker: ignore-file[smell-clone-reflex]
// Snapshot views are owned read-model projections from borrowed symbol records.
use rustc_hash::FxHashMap;

use super::model::{SymbolDetail, SymbolSummary};
use crate::snapshot::model::{ChangeStatus, SymbolId, SymbolRecord, WorkspaceSnapshot};

pub struct SymbolView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> SymbolView<'a> {
	pub(super) fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn all(&self) -> Vec<SymbolSummary> {
		let counts = self.navigable_child_counts();
		let mut symbols = self
			.snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.navigable)
			.map(|symbol| self.summary_with_children(symbol, &counts))
			.collect::<Vec<_>>();
		symbols.sort_by(|left, right| {
			left.source
				.cmp(&right.source)
				.then_with(|| left.identity.cmp(&right.identity))
		});
		symbols
	}

	fn navigable_child_counts(&self) -> FxHashMap<&SymbolId, usize> {
		let mut counts = FxHashMap::default();
		for symbol in self.snapshot.index.symbols.iter() {
			if symbol.navigable
				&& let Some(parent) = symbol.parent.as_ref()
			{
				*counts.entry(parent).or_default() += 1;
			}
		}
		counts
	}

	fn summary_with_children(
		&self,
		symbol: &SymbolRecord,
		counts: &FxHashMap<&SymbolId, usize>,
	) -> SymbolSummary {
		SymbolSummary {
			id: symbol.id,
			source: symbol.source,
			identity: symbol.identity.clone(),
			name: symbol.name.clone(),
			kind: symbol.kind.clone(),
			line_range: symbol.line_range,
			child_count: counts.get(&symbol.id).copied().unwrap_or(0),
			change: self.change_status_for_symbol(&symbol.id),
		}
	}

	pub fn roots_for_source(&self, source: &crate::snapshot::SourceId) -> Vec<SymbolSummary> {
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

	pub fn find(&self, id: &SymbolId) -> Option<&'a SymbolRecord> {
		let record = self
			.snapshot
			.index
			.symbols
			.file_records(id.file())
			.get(id.def());
		if let Some(record) = record
			&& &record.id == id
		{
			return Some(record);
		}
		self.snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| &symbol.id == id)
	}

	pub(super) fn summary(&self, symbol: &SymbolRecord) -> SymbolSummary {
		SymbolSummary {
			id: symbol.id,
			source: symbol.source,
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
