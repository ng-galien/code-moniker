mod catalog;
mod facets;
mod set;

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
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
	compact_identities: FxHashMap<u64, SymbolOrdinal>,
	compact_identity_collisions: FxHashMap<u64, Vec<SymbolOrdinal>>,
}

impl Default for SymbolInventoryIndex {
	fn default() -> Self {
		Self::empty(ResourceGeneration::new(0))
	}
}

impl SymbolInventoryIndex {
	pub(crate) fn estimated_heap_bytes(&self) -> usize {
		let mut strings = std::collections::HashSet::<(usize, usize)>::new();
		let mut string_bytes = 0usize;
		let mut segment_bytes = 0usize;
		for record in self.records.values() {
			for value in [
				&record.identity,
				&record.name,
				&record.kind,
				&record.shape,
				&record.visibility,
				&record.language,
				&record.source_path,
				&record.srcset,
			] {
				if strings.insert((value.as_ptr() as usize, value.len())) {
					string_bytes += value.len();
				}
			}
			segment_bytes += record.segments.len() * std::mem::size_of::<InventorySegment>();
			for segment in record.segments.iter() {
				for value in [&segment.kind, &segment.name] {
					if strings.insert((value.as_ptr() as usize, value.len())) {
						string_bytes += value.len();
					}
				}
			}
		}
		self.records.capacity()
			* (std::mem::size_of::<SymbolOrdinal>() + std::mem::size_of::<InventorySymbol>())
			+ self.catalog.estimated_heap_bytes()
			+ self.all_symbols.estimated_heap_bytes()
			+ self.facets.estimated_heap_bytes()
			+ self.compact_identities.capacity()
				* (std::mem::size_of::<u64>() + std::mem::size_of::<SymbolOrdinal>())
			+ self
				.compact_identity_collisions
				.values()
				.map(|ordinals| ordinals.capacity() * std::mem::size_of::<SymbolOrdinal>())
				.sum::<usize>()
			+ self.compact_identity_collisions.capacity()
				* (std::mem::size_of::<u64>() + std::mem::size_of::<Vec<SymbolOrdinal>>())
			+ segment_bytes
			+ string_bytes
	}

	pub fn empty(generation: ResourceGeneration) -> Self {
		Self {
			generation,
			catalog: Arc::new(SymbolOrdinalCatalog::default()),
			records: FxHashMap::default(),
			all_symbols: SymbolSet::new(),
			facets: SymbolInventoryFacets::default(),
			compact_identities: FxHashMap::default(),
			compact_identity_collisions: FxHashMap::default(),
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
			index_record(&mut inventory, symbol, source, None);
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

	pub fn symbol_ids_by_identity(&self, identity: &str) -> Vec<SymbolId> {
		self.facets
			.symbols_by_identity(identity)
			.into_iter()
			.flat_map(SymbolSet::iter)
			.filter_map(|ordinal| self.catalog.id(ordinal).copied())
			.collect()
	}

	pub fn symbol_ids_by_compact_identity(&self, compact: &str) -> Vec<SymbolId> {
		let hash = compact_identity_hash(compact);
		let candidates = self
			.compact_identity_collisions
			.get(&hash)
			.map(Vec::as_slice)
			.or_else(|| self.compact_identities.get(&hash).map(std::slice::from_ref))
			.unwrap_or_default();
		candidates
			.iter()
			.copied()
			.filter_map(|ordinal| self.record(ordinal))
			.filter(|record| {
				compact_record_identity(record.identity.as_ref()).as_deref() == Some(compact)
			})
			.map(|record| record.id)
			.collect()
	}

	pub fn facets(&self) -> &SymbolInventoryFacets {
		&self.facets
	}
}

fn index_record(
	inventory: &mut SymbolInventoryIndex,
	symbol: &SymbolRecord,
	source: &SourceFileRecord,
	preferred_ordinal: Option<SymbolOrdinal>,
) {
	let catalog = Arc::make_mut(&mut inventory.catalog);
	let ordinal = match preferred_ordinal {
		Some(ordinal) => {
			catalog.bind_id(ordinal, symbol.id);
			ordinal
		}
		None => catalog.push(symbol.id),
	};
	if inventory.records.contains_key(&ordinal) {
		unindex_record(inventory, ordinal);
	}
	let record = inventory_record(symbol, source);
	inventory.all_symbols.insert(ordinal);
	facets::insert_facets(&mut inventory.facets, &record, ordinal);
	if let Some(compact) = compact_record_identity(record.identity.as_ref()) {
		index_compact_identity(inventory, compact_identity_hash(&compact), ordinal);
	}
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
	if let Some(compact) = compact_record_identity(record.identity.as_ref()) {
		let hash = compact_identity_hash(&compact);
		unindex_compact_identity(inventory, hash, ordinal);
	}
}

fn index_compact_identity(inventory: &mut SymbolInventoryIndex, hash: u64, ordinal: SymbolOrdinal) {
	let Some(existing) = inventory.compact_identities.get(&hash).copied() else {
		inventory.compact_identities.insert(hash, ordinal);
		return;
	};
	let collisions = inventory
		.compact_identity_collisions
		.entry(hash)
		.or_insert_with(|| vec![existing]);
	if !collisions.contains(&ordinal) {
		collisions.push(ordinal);
	}
}

fn unindex_compact_identity(
	inventory: &mut SymbolInventoryIndex,
	hash: u64,
	ordinal: SymbolOrdinal,
) {
	if let Some(collisions) = inventory.compact_identity_collisions.get_mut(&hash) {
		collisions.retain(|candidate| *candidate != ordinal);
		match collisions.as_slice() {
			[] => {
				inventory.compact_identities.remove(&hash);
				inventory.compact_identity_collisions.remove(&hash);
			}
			[remaining] => {
				inventory.compact_identities.insert(hash, *remaining);
				inventory.compact_identity_collisions.remove(&hash);
			}
			_ => {}
		}
	} else if inventory.compact_identities.get(&hash) == Some(&ordinal) {
		inventory.compact_identities.remove(&hash);
	}
}

fn compact_identity_hash(compact: &str) -> u64 {
	let mut hasher = rustc_hash::FxHasher::default();
	compact.hash(&mut hasher);
	hasher.finish()
}

fn compact_record_identity(identity: &str) -> Option<String> {
	let scheme_end = identity.find("://")? + 3;
	let scheme = &identity[..scheme_end];
	crate::code::compact_identity(identity, scheme)
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
	let mut reusable_ordinals = FxHashMap::<(SourceId, Arc<str>), Vec<SymbolOrdinal>>::default();
	for ordinal in &pending_retire {
		if let Some(record) = inventory.record(*ordinal) {
			reusable_ordinals
				.entry((record.source, Arc::clone(&record.identity)))
				.or_default()
				.push(*ordinal);
		}
	}
	for ordinal in &pending_retire {
		unindex_record(&mut inventory, *ordinal);
		Arc::make_mut(&mut inventory.catalog).unbind_id(*ordinal);
	}
	for file in changed_files {
		for symbol in symbols.file_records(*file) {
			let fallback = missing_source(symbol.source);
			let source = sources.get(*file).unwrap_or(&fallback);
			let key = (symbol.source, Arc::clone(&symbol.identity));
			let preferred = reusable_ordinals.get_mut(&key).and_then(Vec::pop);
			index_record(&mut inventory, symbol, source, preferred);
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
