use std::path::PathBuf;

use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;

use crate::code::{NormalizedSource, NormalizedSymbol, SymbolProvider};
use crate::snapshot::{
	ChangeId, ChangeRecord, ChangeRecordCoreFields, ChangeStatus, SymbolLocation,
};

use super::diff;

pub struct ChangeAnalyzer<'a, P> {
	symbols: &'a P,
}

impl<'a, P> ChangeAnalyzer<'a, P>
where
	P: SymbolProvider,
{
	pub fn new(symbols: &'a P) -> Self {
		Self { symbols }
	}

	pub fn analyze(&self, entries: &[diff::ChangeEntry]) -> Vec<ChangeRecord> {
		entries
			.iter()
			.enumerate()
			.map(|(idx, entry)| PendingChange::from(entry).record(idx, self.symbols))
			.collect()
	}
}

struct PendingChange {
	loc: Option<SymbolLocation>,
	status: ChangeStatus,
	lang: Lang,
	file_path: PathBuf,
	kind: String,
	name: String,
	moniker: Moniker,
	hunk_count: usize,
	line_range: Option<(u32, u32)>,
}

impl PendingChange {
	fn record<P>(&self, idx: usize, symbols: &P) -> ChangeRecord
	where
		P: SymbolProvider,
	{
		let symbol = self.symbol(symbols);
		let source = self.source(symbols, symbol.as_ref());
		let mut record = ChangeRecord::new(ChangeRecordCoreFields {
			id: ChangeId::new(format!("change:{idx}")),
			status: self.status,
			identity: self.identity(symbols, symbol.as_ref()),
			language: self.language(source.as_ref()),
			file_path: self.display_path(source.as_ref()),
			name: self.name.clone(),
			kind: self.kind.clone(),
			line_range: self.line_range,
			hunk_count: self.hunk_count,
		});
		if let Some(source) = source {
			record = record.with_source(source.id, source.uri);
		}
		if let Some(symbol) = symbol {
			record = record.with_symbol(symbol.id);
		}
		record
	}

	fn symbol<P>(&self, symbols: &P) -> Option<NormalizedSymbol>
	where
		P: SymbolProvider,
	{
		self.loc
			.and_then(|loc| symbols.symbol_at(loc))
			.or_else(|| symbols.symbol_for_moniker(&self.moniker))
	}

	fn source<P>(&self, symbols: &P, symbol: Option<&NormalizedSymbol>) -> Option<NormalizedSource>
	where
		P: SymbolProvider,
	{
		self.loc
			.and_then(|loc| symbols.source_at(loc.file))
			.or_else(|| symbol.map(|symbol| symbol.source.clone()))
	}

	fn identity<P>(&self, symbols: &P, symbol: Option<&NormalizedSymbol>) -> String
	where
		P: SymbolProvider,
	{
		symbol
			.map(|symbol| symbol.identity.clone())
			.unwrap_or_else(|| symbols.identity_for_moniker(&self.moniker))
	}

	fn language(&self, source: Option<&NormalizedSource>) -> String {
		source
			.map(NormalizedSource::language_tag)
			.unwrap_or_else(|| self.lang.tag().to_string())
	}

	fn display_path(&self, source: Option<&NormalizedSource>) -> String {
		source
			.map(NormalizedSource::display_path)
			.unwrap_or_else(|| self.file_path.display().to_string())
	}
}

impl NormalizedSource {
	fn language_tag(&self) -> String {
		self.language.tag().to_string()
	}

	fn display_path(&self) -> String {
		self.rel_path.display().to_string()
	}
}

fn change_status(status: diff::ChangeStatus) -> ChangeStatus {
	match status {
		diff::ChangeStatus::Added => ChangeStatus::Added,
		diff::ChangeStatus::Modified => ChangeStatus::Modified,
		diff::ChangeStatus::Removed => ChangeStatus::Removed,
	}
}

impl From<&diff::ChangeEntry> for PendingChange {
	fn from(entry: &diff::ChangeEntry) -> Self {
		Self {
			loc: entry.loc,
			status: change_status(entry.status),
			lang: entry.lang,
			file_path: entry.file_path.clone(),
			kind: entry.kind.clone(),
			name: entry.name.clone(),
			moniker: entry.moniker.clone(),
			hunk_count: entry.hunk_count,
			line_range: entry.line_range,
		}
	}
}
