mod catalog;
mod facets;
mod set;

use std::collections::BTreeSet;
use std::sync::Arc;

use code_moniker_core::core::shape::Shape;
use code_moniker_core::core::uri::{UriConfig, from_uri};
use rustc_hash::FxHashMap;

use super::{RecordTable, ResourceGeneration, SourceFileRecord, SourceId, SymbolId, SymbolRecord};

pub use catalog::SymbolOrdinalCatalog;
pub use facets::SymbolInventoryFacets;
pub use set::SymbolSet;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SymbolOrdinal(u32);

impl SymbolOrdinal {
	pub fn from_index(value: usize) -> Self {
		assert!(
			u32::try_from(value).is_ok(),
			"symbol index exceeds u32 range"
		);
		Self(value as u32)
	}

	pub fn as_usize(self) -> usize {
		self.0 as usize
	}

	pub fn raw(self) -> u32 {
		self.0
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InventorySegment {
	pub kind: Arc<str>,
	pub name: Arc<str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventorySymbol {
	pub id: SymbolId,
	pub source: SourceId,
	pub identity: Arc<str>,
	pub name: Arc<str>,
	pub kind: Arc<str>,
	pub shape: Arc<str>,
	pub visibility: Arc<str>,
	pub language: Arc<str>,
	pub source_path: Arc<str>,
	pub source_root: usize,
	pub srcset: Arc<str>,
	pub line_range: Option<(u32, u32)>,
	pub parent: Option<SymbolId>,
	pub segments: Arc<[InventorySegment]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolInventoryIndex {
	generation: ResourceGeneration,
	catalog: Arc<SymbolOrdinalCatalog>,
	records: FxHashMap<SymbolOrdinal, InventorySymbol>,
	all_symbols: SymbolSet,
	facets: SymbolInventoryFacets,
}

impl Default for SymbolInventoryIndex {
	fn default() -> Self {
		Self::empty(ResourceGeneration::new(0))
	}
}

impl SymbolInventoryIndex {
	pub fn empty(generation: ResourceGeneration) -> Self {
		Self {
			generation,
			catalog: Arc::new(SymbolOrdinalCatalog::default()),
			records: FxHashMap::default(),
			all_symbols: SymbolSet::new(),
			facets: SymbolInventoryFacets::default(),
		}
	}

	pub fn build(
		generation: ResourceGeneration,
		sources: &[SourceFileRecord],
		symbols: &RecordTable<SymbolRecord>,
	) -> Self {
		let mut inventory = Self::empty(generation);
		for symbol in symbols.iter() {
			let fallback = missing_source(symbol.source);
			let source = sources.get(symbol.source.file()).unwrap_or(&fallback);
			index_record(&mut inventory, symbol, source);
		}
		inventory
	}

	pub fn refresh(
		&self,
		generation: ResourceGeneration,
		sources: &[SourceFileRecord],
		symbols: &RecordTable<SymbolRecord>,
		changed_files: &BTreeSet<usize>,
	) -> Self {
		refresh_inventory(self, generation, sources, symbols, changed_files)
	}

	pub fn generation(&self) -> ResourceGeneration {
		self.generation
	}

	pub fn catalog(&self) -> &Arc<SymbolOrdinalCatalog> {
		&self.catalog
	}

	pub fn all_symbols(&self) -> &SymbolSet {
		&self.all_symbols
	}

	pub fn record(&self, ordinal: SymbolOrdinal) -> Option<&InventorySymbol> {
		self.records.get(&ordinal)
	}

	pub fn record_by_id(&self, id: &SymbolId) -> Option<&InventorySymbol> {
		self.catalog
			.ordinal(id)
			.and_then(|ordinal| self.record(ordinal))
	}

	pub fn facets(&self) -> &SymbolInventoryFacets {
		&self.facets
	}
}

fn index_record(
	inventory: &mut SymbolInventoryIndex,
	symbol: &SymbolRecord,
	source: &SourceFileRecord,
) {
	let ordinal =
		Arc::make_mut(&mut inventory.catalog).push(symbol.id, Arc::clone(&symbol.identity));
	if inventory.records.contains_key(&ordinal) {
		unindex_record(inventory, ordinal);
	}
	let record = inventory_record(symbol, source);
	inventory.all_symbols.insert(ordinal);
	facets::insert_facets(&mut inventory.facets, &record, ordinal);
	inventory.records.insert(ordinal, record);
}

fn inventory_record(symbol: &SymbolRecord, source: &SourceFileRecord) -> InventorySymbol {
	let segments = parse_segments(&symbol.identity);
	let srcset = segments
		.iter()
		.filter(|segment| segment.kind.as_ref() == "srcset")
		.map(|segment| segment.name.as_ref())
		.collect::<Vec<_>>()
		.join(".");
	InventorySymbol {
		id: symbol.id,
		source: symbol.source,
		identity: Arc::clone(&symbol.identity),
		name: Arc::from(symbol.name.as_str()),
		kind: Arc::from(symbol.kind.as_str()),
		shape: Arc::from(Shape::for_kind(symbol.kind.as_bytes()).as_str()),
		visibility: Arc::from(symbol.visibility.as_str()),
		language: Arc::from(source.language.as_str()),
		source_path: Arc::from(source.path.as_str()),
		source_root: source.source_root,
		srcset: Arc::from(srcset),
		line_range: symbol.line_range,
		parent: symbol.parent,
		segments: Arc::from(segments),
	}
}

fn parse_segments(identity: &str) -> Vec<InventorySegment> {
	let marker = "+moniker://";
	let scheme_end = identity.find(marker).map(|index| index + marker.len());
	let Some(scheme) = scheme_end.and_then(|end| identity.get(..end)) else {
		return Vec::new();
	};
	let Ok(moniker) = from_uri(identity, &UriConfig { scheme }) else {
		return Vec::new();
	};
	moniker
		.as_view()
		.segments()
		.filter_map(|segment| {
			Some(InventorySegment {
				kind: Arc::from(std::str::from_utf8(segment.kind).ok()?),
				name: Arc::from(std::str::from_utf8(segment.name).ok()?),
			})
		})
		.collect()
}

fn unindex_record(inventory: &mut SymbolInventoryIndex, ordinal: SymbolOrdinal) {
	let Some(record) = inventory.records.remove(&ordinal) else {
		return;
	};
	inventory.all_symbols.remove(ordinal);
	facets::remove_facets(&mut inventory.facets, &record, ordinal);
}

fn refresh_inventory(
	previous: &SymbolInventoryIndex,
	generation: ResourceGeneration,
	sources: &[SourceFileRecord],
	symbols: &RecordTable<SymbolRecord>,
	changed_files: &BTreeSet<usize>,
) -> SymbolInventoryIndex {
	let mut inventory = previous.clone();
	inventory.generation = generation;
	let pending_retire = changed_ordinals(&inventory, changed_files);
	for ordinal in &pending_retire {
		unindex_record(&mut inventory, *ordinal);
		Arc::make_mut(&mut inventory.catalog).unbind_id(*ordinal);
	}
	for file in changed_files {
		for symbol in symbols.file_records(*file) {
			let fallback = missing_source(symbol.source);
			let source = sources.get(*file).unwrap_or(&fallback);
			index_record(&mut inventory, symbol, source);
		}
	}
	for ordinal in pending_retire {
		if inventory.catalog.id(ordinal).is_none() {
			Arc::make_mut(&mut inventory.catalog).retire(ordinal);
		}
	}
	inventory
}

fn missing_source(source: SourceId) -> SourceFileRecord {
	SourceFileRecord {
		id: source,
		uri: String::new(),
		source_root: 0,
		path: String::new(),
		rel_path: String::new(),
		anchor: String::new(),
		language: String::new(),
		text: String::new(),
	}
}

fn changed_ordinals(
	inventory: &SymbolInventoryIndex,
	changed_files: &BTreeSet<usize>,
) -> Vec<SymbolOrdinal> {
	let mut changed = SymbolSet::new();
	for file in changed_files {
		if let Some(symbols) = inventory.facets.symbols_by_source(SourceId::at(*file)) {
			changed.union_with(symbols);
		}
	}
	changed.iter().collect()
}

#[cfg(test)]
mod tests;
