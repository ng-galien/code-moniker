use std::path::PathBuf;

use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;

use crate::workspace::index::DefLocation;
use crate::workspace::resources::material::CodeIndexMaterial;
use crate::workspace::session::{SourceId, SymbolId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NormalizedSource {
	pub(super) id: SourceId,
	pub(super) uri: String,
	pub(super) language: Lang,
	pub(super) rel_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NormalizedSymbol {
	pub(super) id: SymbolId,
	pub(super) source: NormalizedSource,
	pub(super) identity: String,
}

pub(super) trait SymbolProvider {
	fn source_at(&self, file_idx: usize) -> Option<NormalizedSource>;
	fn symbol_at(&self, loc: DefLocation) -> Option<NormalizedSymbol>;
	fn symbol_for_moniker(&self, moniker: &Moniker) -> Option<NormalizedSymbol>;
	fn identity_for_moniker(&self, moniker: &Moniker) -> String;
}

pub(super) struct CodeIndexSymbolProvider<'a> {
	material: &'a CodeIndexMaterial,
}

impl<'a> CodeIndexSymbolProvider<'a> {
	pub(super) fn new(material: &'a CodeIndexMaterial) -> Self {
		Self { material }
	}
}

impl SymbolProvider for CodeIndexSymbolProvider<'_> {
	fn source_at(&self, file_idx: usize) -> Option<NormalizedSource> {
		let file = self.material.files.get(file_idx)?;
		Some(NormalizedSource {
			id: file.source_id.clone(),
			uri: file.source_uri.clone(),
			language: file.lang,
			rel_path: file.rel_path.clone(),
		})
	}

	fn symbol_at(&self, loc: DefLocation) -> Option<NormalizedSymbol> {
		let file = self.material.files.get(loc.file)?;
		let def = file.graph.defs().nth(loc.def)?;
		let source = self.source_at(loc.file)?;
		Some(NormalizedSymbol {
			id: file.identity.symbol_id(loc.file, loc.def),
			source,
			identity: self.material.identity.moniker_uri(&def.moniker),
		})
	}

	fn symbol_for_moniker(&self, moniker: &Moniker) -> Option<NormalizedSymbol> {
		let id = self.material.symbols_by_moniker.get(moniker)?;
		let (file, def) = self.material.identity.symbol_location(id)?;
		self.symbol_at(DefLocation { file, def })
	}

	fn identity_for_moniker(&self, moniker: &Moniker) -> String {
		self.material.identity.moniker_uri(moniker)
	}
}
