use std::path::PathBuf;

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;

use crate::cache;
use crate::workspace::snapshot::{SourceId, SymbolId, SymbolLocation};
use crate::workspace::source::{CodeIndexMaterial, ResolvedSourceResource};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NormalizedSource {
	pub(crate) id: SourceId,
	pub(crate) uri: String,
	pub(crate) language: Lang,
	pub(crate) rel_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NormalizedSymbol {
	pub(crate) id: SymbolId,
	pub(crate) source: NormalizedSource,
	pub(crate) identity: String,
}

#[allow(dead_code)]
pub(crate) trait SymbolProvider {
	fn source_at(&self, file_idx: usize) -> Option<NormalizedSource>;
	fn source_for_path(&self, path: &std::path::Path) -> Option<NormalizedSource>;
	fn symbol_at(&self, loc: SymbolLocation) -> Option<NormalizedSymbol>;
	fn symbol_for_moniker(&self, moniker: &Moniker) -> Option<NormalizedSymbol>;
	fn symbols_for_path(&self, path: &std::path::Path) -> Option<Vec<NormalizedSymbol>>;
	fn identity_for_moniker(&self, moniker: &Moniker) -> String;
}

pub(crate) struct CodeIndexSymbolProvider<'a> {
	material: &'a CodeIndexMaterial,
}

impl<'a> CodeIndexSymbolProvider<'a> {
	pub(crate) fn new(material: &'a CodeIndexMaterial) -> Self {
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

	fn source_for_path(&self, path: &std::path::Path) -> Option<NormalizedSource> {
		let source = self.material.source_catalog.resolve_source(path)?;
		Some(normalized_source(source))
	}

	fn symbol_at(&self, loc: SymbolLocation) -> Option<NormalizedSymbol> {
		let file = self.material.files.get(loc.file)?;
		let def = file.graph.defs().nth(loc.symbol)?;
		let source = self.source_at(loc.file)?;
		Some(NormalizedSymbol {
			id: file.identity.symbol_id(loc.file, loc.symbol),
			source,
			identity: self.material.identity.moniker_uri(&def.moniker),
		})
	}

	fn symbol_for_moniker(&self, moniker: &Moniker) -> Option<NormalizedSymbol> {
		let id = self.material.symbols_by_moniker.get(moniker)?;
		let (file, symbol) = self.material.identity.symbol_location(id)?;
		self.symbol_at(SymbolLocation { file, symbol })
	}

	fn symbols_for_path(&self, path: &std::path::Path) -> Option<Vec<NormalizedSymbol>> {
		SourceSymbolLookup::new(self.material).symbols_for_path(path)
	}

	fn identity_for_moniker(&self, moniker: &Moniker) -> String {
		self.material.identity.moniker_uri(moniker)
	}
}

#[allow(dead_code)]
struct SourceSymbolLookup<'a> {
	material: &'a CodeIndexMaterial,
}

impl<'a> SourceSymbolLookup<'a> {
	fn new(material: &'a CodeIndexMaterial) -> Self {
		Self { material }
	}

	fn symbols_for_path(&self, path: &std::path::Path) -> Option<Vec<NormalizedSymbol>> {
		let source = self.material.source_catalog.resolve_source(path)?;
		match source.eager_index {
			Some(file_idx) => self.indexed_symbols(source, file_idx),
			None => self.lazy_symbols(source),
		}
	}

	fn indexed_symbols(
		&self,
		source: ResolvedSourceResource,
		file_idx: usize,
	) -> Option<Vec<NormalizedSymbol>> {
		Some(self.symbols_from_defs(&source, self.material.files.get(file_idx)?.graph.defs()))
	}

	fn lazy_symbols(&self, source: ResolvedSourceResource) -> Option<Vec<NormalizedSymbol>> {
		let root = self
			.material
			.source_catalog
			.sources
			.roots
			.get(source.source_root)?;
		let (graph, _) = cache::load_or_extract_result(
			&source.path,
			&source.anchor,
			source.lang,
			None,
			&root.ctx,
		)
		.ok()?;
		Some(self.symbols_from_defs(&source, graph.defs()))
	}

	fn symbols_from_defs<'d>(
		&self,
		source: &ResolvedSourceResource,
		defs: impl Iterator<Item = &'d DefRecord>,
	) -> Vec<NormalizedSymbol> {
		defs.enumerate()
			.map(|(def_idx, def)| self.normalized_symbol_for_def(source, def_idx, def))
			.collect()
	}

	fn normalized_symbol_for_def(
		&self,
		source: &ResolvedSourceResource,
		def_idx: usize,
		def: &DefRecord,
	) -> NormalizedSymbol {
		NormalizedSymbol {
			id: lazy_or_eager_symbol_id(source, def_idx, &self.material.identity),
			source: normalized_source(source.clone()),
			identity: self.material.identity.moniker_uri(&def.moniker),
		}
	}
}

#[allow(dead_code)]
fn normalized_source(source: ResolvedSourceResource) -> NormalizedSource {
	NormalizedSource {
		id: source.source_id,
		uri: source.source_uri,
		language: source.lang,
		rel_path: source.rel_path,
	}
}

#[allow(dead_code)]
fn lazy_or_eager_symbol_id(
	source: &ResolvedSourceResource,
	def_idx: usize,
	identity: &crate::workspace::source::LocalIdentityResolver,
) -> SymbolId {
	match source.eager_index {
		Some(file_idx) => identity.symbol_id(file_idx, def_idx),
		None => SymbolId::new(format!(
			"symbol:lazy:{}:{def_idx}",
			source.rel_path.display()
		)),
	}
}

pub(crate) fn is_navigable_def(lang: Lang, def: &DefRecord) -> bool {
	lang.kind_spec(&def_kind(def)).is_some()
}

pub(crate) fn def_kind(def: &DefRecord) -> String {
	std::str::from_utf8(&def.kind).unwrap_or("?").to_string()
}

pub(crate) fn ref_kind(reference: &RefRecord) -> String {
	std::str::from_utf8(&reference.kind)
		.unwrap_or("?")
		.to_string()
}

pub(crate) fn last_name(moniker: &Moniker) -> String {
	moniker
		.as_view()
		.segments()
		.last()
		.and_then(|s| std::str::from_utf8(s.name).ok())
		.unwrap_or(".")
		.to_string()
}

pub(crate) fn compact_moniker(moniker: &Moniker) -> String {
	crate::format::render_compact_moniker(moniker, false).unwrap_or_else(|| {
		let cfg = code_moniker_core::core::uri::UriConfig {
			scheme: crate::DEFAULT_SCHEME,
		};
		crate::render_uri(moniker, &cfg)
	})
}
