use std::path::PathBuf;

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;

use crate::environment;
use crate::snapshot::{SourceId, SymbolId, SymbolLocation};
use crate::source::CodeIndexMaterial;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedSource {
	pub id: SourceId,
	pub uri: String,
	pub language: Lang,
	pub rel_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedSymbol {
	pub id: SymbolId,
	pub source: NormalizedSource,
	pub identity: String,
}

pub struct CodeIndexSymbolProvider<'a> {
	material: &'a CodeIndexMaterial,
}

impl<'a> CodeIndexSymbolProvider<'a> {
	pub fn new(material: &'a CodeIndexMaterial) -> Self {
		Self { material }
	}

	pub fn source_at(&self, file_idx: usize) -> Option<NormalizedSource> {
		let file = self.material.files.get(file_idx)?;
		Some(NormalizedSource {
			id: file.source_id.clone(),
			uri: file.source_uri.clone(),
			language: file.lang,
			rel_path: file.rel_path.clone(),
		})
	}

	pub fn symbol_at(&self, loc: SymbolLocation) -> Option<NormalizedSymbol> {
		let file = self.material.files.get(loc.file)?;
		let def = file.graph.defs().nth(loc.symbol)?;
		let source = self.source_at(loc.file)?;
		Some(NormalizedSymbol {
			id: file.identity.symbol_id(loc.file, loc.symbol),
			source,
			identity: self.material.identity.moniker_uri(&def.moniker),
		})
	}

	pub fn symbol_for_moniker(&self, moniker: &Moniker) -> Option<NormalizedSymbol> {
		let id = self.material.symbols_by_moniker.get(moniker)?;
		let (file, symbol) = self.material.identity.symbol_location(id)?;
		self.symbol_at(SymbolLocation { file, symbol })
	}

	pub fn identity_for_moniker(&self, moniker: &Moniker) -> String {
		self.material.identity.moniker_uri(moniker)
	}
}

pub fn is_navigable_def(lang: Lang, def: &DefRecord) -> bool {
	lang.kind_spec(&def_kind(def)).is_some()
}

pub fn def_kind(def: &DefRecord) -> String {
	std::str::from_utf8(&def.kind).unwrap_or("?").to_string()
}

pub fn ref_kind(reference: &RefRecord) -> String {
	std::str::from_utf8(&reference.kind)
		.unwrap_or("?")
		.to_string()
}

pub fn last_name(moniker: &Moniker) -> String {
	moniker
		.as_view()
		.segments()
		.last()
		.and_then(|s| std::str::from_utf8(s.name).ok())
		.unwrap_or(".")
		.to_string()
}

pub fn compact_moniker(moniker: &Moniker) -> String {
	environment::compact_moniker(moniker, crate::DEFAULT_IDENTITY_SCHEME)
}
