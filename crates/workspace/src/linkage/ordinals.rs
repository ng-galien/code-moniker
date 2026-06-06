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
	ordinals_by_id: FxHashMap<SymbolId, SymbolOrdinal>,
}

impl SymbolOrdinalCatalog {
	pub(super) fn push(&mut self, id: SymbolId) -> SymbolOrdinal {
		let ordinal = SymbolOrdinal::from_index(self.ids.len());
		self.ordinals_by_id.insert(id.clone(), ordinal);
		self.ids.push(id);
		ordinal
	}

	pub(super) fn id(&self, ordinal: SymbolOrdinal) -> Option<&SymbolId> {
		self.ids.get(ordinal.index())
	}

	pub(super) fn len(&self) -> usize {
		self.ids.len()
	}

	pub(super) fn ordinal(&self, id: &SymbolId) -> Option<SymbolOrdinal> {
		self.ordinals_by_id.get(id).copied()
	}

	pub(super) fn has_same_order(&self, other: &Self) -> bool {
		self.ids == other.ids
	}

	pub(super) fn remap_set_with_ids(
		&self,
		symbols: &SymbolSet,
		next: &Self,
		id_remaps: &FxHashMap<SymbolId, SymbolId>,
	) -> SymbolSet {
		symbols
			.iter()
			.filter_map(|symbol| self.id(symbol))
			.filter_map(|id| {
				next.ordinal(id)
					.or_else(|| id_remaps.get(id).and_then(|next_id| next.ordinal(next_id)))
			})
			.collect()
	}

	pub(super) fn ids(&self, symbols: &SymbolSet) -> Vec<SymbolId> {
		symbols
			.iter()
			.filter_map(|symbol| self.id(symbol).cloned())
			.collect()
	}
}
