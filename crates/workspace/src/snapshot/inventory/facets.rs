use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{InventorySegment, InventorySymbol, SourceId, SymbolOrdinal, SymbolSet};

// Posting-list accessors are independent projections over one immutable
// inventory; low field overlap is the intended shape of this index.
// code-moniker: ignore[smell-god-type-local-metrics]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SymbolInventoryFacets {
	by_identity: FxHashMap<Arc<str>, SymbolSet>,
	by_name: FxHashMap<Arc<str>, SymbolSet>,
	by_kind: FxHashMap<Arc<str>, SymbolSet>,
	by_shape: FxHashMap<Arc<str>, SymbolSet>,
	by_visibility: FxHashMap<Arc<str>, SymbolSet>,
	by_language: FxHashMap<Arc<str>, SymbolSet>,
	by_source: FxHashMap<SourceId, SymbolSet>,
	by_source_path: FxHashMap<Arc<str>, SymbolSet>,
	by_source_root: FxHashMap<usize, SymbolSet>,
	by_srcset: FxHashMap<Arc<str>, SymbolSet>,
	by_segment: FxHashMap<InventorySegment, SymbolSet>,
}

impl SymbolInventoryFacets {
	pub(super) fn estimated_heap_bytes(&self) -> usize {
		string_postings_bytes(&self.by_identity)
			+ string_postings_bytes(&self.by_name)
			+ string_postings_bytes(&self.by_kind)
			+ string_postings_bytes(&self.by_shape)
			+ string_postings_bytes(&self.by_visibility)
			+ string_postings_bytes(&self.by_language)
			+ postings_bytes(&self.by_source)
			+ string_postings_bytes(&self.by_source_path)
			+ postings_bytes(&self.by_source_root)
			+ string_postings_bytes(&self.by_srcset)
			+ postings_bytes(&self.by_segment)
	}

	pub fn symbols_by_identity(&self, identity: &str) -> Option<&SymbolSet> {
		self.by_identity.get(identity)
	}

	pub fn symbols_by_name(&self, name: &str) -> Option<&SymbolSet> {
		self.by_name.get(name)
	}

	pub fn name_postings(&self) -> impl Iterator<Item = (&str, &SymbolSet)> {
		posting_values(&self.by_name)
	}

	pub fn symbols_by_kind(&self, kind: &str) -> Option<&SymbolSet> {
		self.by_kind.get(kind)
	}

	pub fn kind_postings(&self) -> impl Iterator<Item = (&str, &SymbolSet)> {
		posting_values(&self.by_kind)
	}

	pub fn symbols_by_shape(&self, shape: &str) -> Option<&SymbolSet> {
		self.by_shape.get(shape)
	}

	pub fn shape_postings(&self) -> impl Iterator<Item = (&str, &SymbolSet)> {
		posting_values(&self.by_shape)
	}

	pub fn symbols_by_visibility(&self, visibility: &str) -> Option<&SymbolSet> {
		self.by_visibility.get(visibility)
	}

	pub fn visibility_postings(&self) -> impl Iterator<Item = (&str, &SymbolSet)> {
		posting_values(&self.by_visibility)
	}

	pub fn symbols_by_language(&self, language: &str) -> Option<&SymbolSet> {
		self.by_language.get(language)
	}

	pub fn language_postings(&self) -> impl Iterator<Item = (&str, &SymbolSet)> {
		posting_values(&self.by_language)
	}

	pub fn symbols_by_source(&self, source: SourceId) -> Option<&SymbolSet> {
		self.by_source.get(&source)
	}

	pub fn symbols_by_source_path(&self, path: &str) -> Option<&SymbolSet> {
		self.by_source_path.get(path)
	}

	pub fn source_path_postings(&self) -> impl Iterator<Item = (&str, &SymbolSet)> {
		posting_values(&self.by_source_path)
	}

	pub fn symbols_by_source_root(&self, root: usize) -> Option<&SymbolSet> {
		self.by_source_root.get(&root)
	}

	pub fn symbols_by_srcset(&self, srcset: &str) -> Option<&SymbolSet> {
		self.by_srcset.get(srcset)
	}

	pub fn srcset_postings(&self) -> impl Iterator<Item = (&str, &SymbolSet)> {
		posting_values(&self.by_srcset)
	}

	pub fn symbols_by_segment(&self, kind: &str, name: &str) -> Option<&SymbolSet> {
		self.by_segment.get(&InventorySegment {
			kind: Arc::from(kind),
			name: Arc::from(name),
		})
	}

	pub fn segment_postings(&self) -> impl Iterator<Item = (&InventorySegment, &SymbolSet)> {
		self.by_segment.iter()
	}
}

fn postings_bytes<K>(postings: &FxHashMap<K, SymbolSet>) -> usize {
	postings.capacity() * (std::mem::size_of::<K>() + std::mem::size_of::<SymbolSet>())
		+ postings
			.values()
			.map(SymbolSet::estimated_heap_bytes)
			.sum::<usize>()
}

fn string_postings_bytes(postings: &FxHashMap<Arc<str>, SymbolSet>) -> usize {
	postings_bytes(postings)
}

pub(super) fn insert_facets(
	facets: &mut SymbolInventoryFacets,
	record: &InventorySymbol,
	ordinal: SymbolOrdinal,
) {
	insert_posting(
		&mut facets.by_identity,
		Arc::clone(&record.identity),
		ordinal,
	);
	insert_posting(&mut facets.by_name, Arc::clone(&record.name), ordinal);
	insert_posting(&mut facets.by_kind, Arc::clone(&record.kind), ordinal);
	insert_posting(&mut facets.by_shape, Arc::clone(&record.shape), ordinal);
	insert_posting(
		&mut facets.by_visibility,
		Arc::clone(&record.visibility),
		ordinal,
	);
	insert_posting(
		&mut facets.by_language,
		Arc::clone(&record.language),
		ordinal,
	);
	insert_posting(&mut facets.by_source, record.source, ordinal);
	insert_posting(
		&mut facets.by_source_path,
		Arc::clone(&record.source_path),
		ordinal,
	);
	insert_posting(&mut facets.by_source_root, record.source_root, ordinal);
	insert_posting(&mut facets.by_srcset, Arc::clone(&record.srcset), ordinal);
	for segment in record.segments.iter() {
		insert_posting(&mut facets.by_segment, segment.clone(), ordinal);
	}
}

pub(super) fn remove_facets(
	facets: &mut SymbolInventoryFacets,
	record: &InventorySymbol,
	ordinal: SymbolOrdinal,
) {
	remove_posting(&mut facets.by_identity, &record.identity, ordinal);
	remove_posting(&mut facets.by_name, &record.name, ordinal);
	remove_posting(&mut facets.by_kind, &record.kind, ordinal);
	remove_posting(&mut facets.by_shape, &record.shape, ordinal);
	remove_posting(&mut facets.by_visibility, &record.visibility, ordinal);
	remove_posting(&mut facets.by_language, &record.language, ordinal);
	remove_posting(&mut facets.by_source, &record.source, ordinal);
	remove_posting(&mut facets.by_source_path, &record.source_path, ordinal);
	remove_posting(&mut facets.by_source_root, &record.source_root, ordinal);
	remove_posting(&mut facets.by_srcset, &record.srcset, ordinal);
	for segment in record.segments.iter() {
		remove_posting(&mut facets.by_segment, segment, ordinal);
	}
}

fn posting_values(
	index: &FxHashMap<Arc<str>, SymbolSet>,
) -> impl Iterator<Item = (&str, &SymbolSet)> {
	index
		.iter()
		.map(|(value, symbols)| (value.as_ref(), symbols))
}

fn insert_posting<K: Eq + std::hash::Hash>(
	index: &mut FxHashMap<K, SymbolSet>,
	key: K,
	ordinal: SymbolOrdinal,
) {
	index.entry(key).or_default().insert(ordinal);
}

fn remove_posting<K: Eq + std::hash::Hash>(
	index: &mut FxHashMap<K, SymbolSet>,
	key: &K,
	ordinal: SymbolOrdinal,
) {
	if let Some(symbols) = index.get_mut(key) {
		symbols.remove(ordinal);
		if symbols.is_empty() {
			index.remove(key);
		}
	}
}
