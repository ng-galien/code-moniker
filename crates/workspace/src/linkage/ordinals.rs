use roaring::RoaringBitmap;
use rustc_hash::FxHashMap;

use crate::snapshot::SymbolId;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct ReferenceOrdinal(u32);

impl ReferenceOrdinal {
	pub(super) fn from_index(index: usize) -> Self {
		Self(u32::try_from(index).expect("reference index exceeds u32 range"))
	}

	pub(super) fn index(self) -> usize {
		self.0 as usize
	}

	pub(super) fn raw(self) -> u32 {
		self.0
	}
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct SymbolOrdinal(u32);

impl SymbolOrdinal {
	pub(super) fn from_index(index: usize) -> Self {
		Self(u32::try_from(index).expect("symbol index exceeds u32 range"))
	}

	pub(super) fn index(self) -> usize {
		self.0 as usize
	}

	pub(super) fn raw(self) -> u32 {
		self.0
	}
}

#[derive(Clone, Debug, Default)]
pub(super) struct ReferenceSet {
	bitmap: RoaringBitmap,
}

impl ReferenceSet {
	pub(super) fn new() -> Self {
		Self {
			bitmap: RoaringBitmap::new(),
		}
	}

	pub(super) fn insert(&mut self, reference: ReferenceOrdinal) -> bool {
		self.bitmap.insert(reference.raw())
	}

	pub(super) fn contains(&self, reference: ReferenceOrdinal) -> bool {
		self.bitmap.contains(reference.raw())
	}

	pub(super) fn is_empty(&self) -> bool {
		self.bitmap.is_empty()
	}

	pub(super) fn len(&self) -> u64 {
		self.bitmap.len()
	}

	pub(super) fn serialized_size(&self) -> usize {
		self.bitmap.serialized_size()
	}

	pub(super) fn union_with(&mut self, other: &Self) {
		self.bitmap |= &other.bitmap;
	}

	pub(super) fn remove_all(&mut self, stale: &Self) {
		self.bitmap -= &stale.bitmap;
	}

	pub(super) fn iter(&self) -> impl Iterator<Item = ReferenceOrdinal> + '_ {
		self.bitmap.iter().map(ReferenceOrdinal)
	}
}

impl FromIterator<ReferenceOrdinal> for ReferenceSet {
	fn from_iter<T: IntoIterator<Item = ReferenceOrdinal>>(iter: T) -> Self {
		let mut set = Self::new();
		for reference in iter {
			set.insert(reference);
		}
		set
	}
}

#[derive(Clone, Debug, Default)]
pub(super) struct SymbolSet {
	bitmap: RoaringBitmap,
}

impl SymbolSet {
	pub(super) fn new() -> Self {
		Self {
			bitmap: RoaringBitmap::new(),
		}
	}

	pub(super) fn insert(&mut self, symbol: SymbolOrdinal) -> bool {
		self.bitmap.insert(symbol.raw())
	}

	pub(super) fn from_symbol(symbol: SymbolOrdinal) -> Self {
		let mut set = Self::new();
		set.insert(symbol);
		set
	}

	pub(super) fn is_empty(&self) -> bool {
		self.bitmap.is_empty()
	}

	pub(super) fn len(&self) -> usize {
		usize::try_from(self.bitmap.len()).expect("symbol set length exceeds usize")
	}

	pub(super) fn serialized_size(&self) -> usize {
		self.bitmap.serialized_size()
	}

	pub(super) fn iter(&self) -> impl Iterator<Item = SymbolOrdinal> + '_ {
		self.bitmap.iter().map(SymbolOrdinal)
	}

	pub(super) fn single(&self) -> Option<SymbolOrdinal> {
		(self.len() == 1).then(|| self.iter().next()).flatten()
	}
}

impl FromIterator<SymbolOrdinal> for SymbolSet {
	fn from_iter<T: IntoIterator<Item = SymbolOrdinal>>(iter: T) -> Self {
		let mut set = Self::new();
		for symbol in iter {
			set.insert(symbol);
		}
		set
	}
}

#[derive(Clone, Debug, Default)]
pub(super) struct SymbolOrdinalCatalog {
	ids: Vec<SymbolId>,
	identities: Vec<String>,
	ordinals_by_id: FxHashMap<SymbolId, SymbolOrdinal>,
	ordinals_by_identity: FxHashMap<String, SymbolOrdinal>,
}

impl SymbolOrdinalCatalog {
	pub(super) fn push(&mut self, id: SymbolId, identity: String) -> SymbolOrdinal {
		let ordinal = SymbolOrdinal::from_index(self.ids.len());
		self.ordinals_by_id.insert(id.clone(), ordinal);
		self.ordinals_by_identity.insert(identity.clone(), ordinal);
		self.ids.push(id);
		self.identities.push(identity);
		ordinal
	}

	pub(super) fn id(&self, ordinal: SymbolOrdinal) -> Option<&SymbolId> {
		self.ids.get(ordinal.index())
	}

	pub(super) fn identity(&self, ordinal: SymbolOrdinal) -> Option<&str> {
		self.identities.get(ordinal.index()).map(String::as_str)
	}

	pub(super) fn len(&self) -> usize {
		self.ids.len()
	}

	pub(super) fn ordinal(&self, id: &SymbolId) -> Option<SymbolOrdinal> {
		self.ordinals_by_id.get(id).copied()
	}

	pub(super) fn ordinal_by_identity(&self, identity: &str) -> Option<SymbolOrdinal> {
		self.ordinals_by_identity.get(identity).copied()
	}

	pub(super) fn has_same_order(&self, other: &Self) -> bool {
		self.ids == other.ids && self.identities == other.identities
	}

	pub(super) fn remap_set_with_ids(
		&self,
		symbols: &SymbolSet,
		next: &Self,
		_id_remaps: &FxHashMap<SymbolId, SymbolId>,
	) -> SymbolSet {
		SymbolOrdinalRemap::new(self, next).remap_set(symbols)
	}

	pub(super) fn remap_ordinal_with_ids(
		&self,
		symbol: SymbolOrdinal,
		next: &Self,
		_id_remaps: &FxHashMap<SymbolId, SymbolId>,
	) -> Option<SymbolOrdinal> {
		SymbolOrdinalRemap::new(self, next).remap_symbol(symbol)
	}

	pub(super) fn ids(&self, symbols: &SymbolSet) -> Vec<SymbolId> {
		symbols
			.iter()
			.filter_map(|symbol| self.id(symbol).cloned())
			.collect()
	}
}

struct SymbolOrdinalRemap<'a> {
	previous: &'a SymbolOrdinalCatalog,
	next: &'a SymbolOrdinalCatalog,
}

impl<'a> SymbolOrdinalRemap<'a> {
	fn new(previous: &'a SymbolOrdinalCatalog, next: &'a SymbolOrdinalCatalog) -> Self {
		Self { previous, next }
	}

	fn remap_set(&self, symbols: &SymbolSet) -> SymbolSet {
		symbols
			.iter()
			.filter_map(|symbol| self.remap_symbol(symbol))
			.collect()
	}

	fn remap_symbol(&self, symbol: SymbolOrdinal) -> Option<SymbolOrdinal> {
		self.by_identity(symbol)
	}

	fn by_identity(&self, symbol: SymbolOrdinal) -> Option<SymbolOrdinal> {
		self.previous
			.identity(symbol)
			.and_then(|identity| self.next.ordinal_by_identity(identity))
	}
}
