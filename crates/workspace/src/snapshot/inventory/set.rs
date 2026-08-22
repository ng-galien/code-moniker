use roaring::RoaringBitmap;

use super::SymbolOrdinal;

// A bitmap-backed value object intentionally offers the set algebra used by
// linkage and rules; cohesion metrics cannot infer that all methods share the
// wrapped RoaringBitmap through its domain operations.
// code-moniker: ignore[smell-god-type-local-metrics]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SymbolSet {
	bitmap: RoaringBitmap,
}

impl SymbolSet {
	pub fn new() -> Self {
		Self {
			bitmap: RoaringBitmap::new(),
		}
	}

	pub fn insert(&mut self, symbol: SymbolOrdinal) -> bool {
		self.bitmap.insert(symbol.raw())
	}

	pub fn remove(&mut self, symbol: SymbolOrdinal) -> bool {
		self.bitmap.remove(symbol.raw())
	}

	pub fn contains(&self, symbol: SymbolOrdinal) -> bool {
		self.bitmap.contains(symbol.raw())
	}

	pub fn intersects(&self, other: &Self) -> bool {
		!self.bitmap.is_disjoint(&other.bitmap)
	}

	pub fn union_len(&self, other: &Self) -> usize {
		let len = self.bitmap.union_len(&other.bitmap);
		assert!(
			usize::try_from(len).is_ok(),
			"symbol set union length exceeds usize"
		);
		len as usize
	}

	pub fn intersect_with(&mut self, other: &Self) {
		self.bitmap &= &other.bitmap;
	}

	pub fn union_with(&mut self, other: &Self) {
		self.bitmap |= &other.bitmap;
	}

	pub fn remove_all(&mut self, other: &Self) {
		self.bitmap -= &other.bitmap;
	}

	pub fn intersection(&self, other: &Self) -> Self {
		Self {
			bitmap: &self.bitmap & &other.bitmap,
		}
	}

	pub fn union(&self, other: &Self) -> Self {
		Self {
			bitmap: &self.bitmap | &other.bitmap,
		}
	}

	pub fn difference(&self, other: &Self) -> Self {
		let mut result = self.clone();
		result.remove_all(other);
		result
	}

	pub fn from_symbol(symbol: SymbolOrdinal) -> Self {
		let mut set = Self::new();
		set.insert(symbol);
		set
	}

	pub fn is_empty(&self) -> bool {
		self.bitmap.is_empty()
	}

	pub fn serialized_size(&self) -> usize {
		self.bitmap.serialized_size()
	}

	pub(super) fn estimated_heap_bytes(&self) -> usize {
		self.bitmap.serialized_size()
	}

	pub fn single(&self) -> Option<SymbolOrdinal> {
		(self.len() == 1).then(|| self.iter().next()).flatten()
	}

	pub fn len(&self) -> usize {
		let len = self.bitmap.len();
		assert!(
			usize::try_from(len).is_ok(),
			"symbol set length exceeds usize"
		);
		len as usize
	}

	pub fn iter(&self) -> impl Iterator<Item = SymbolOrdinal> + '_ {
		self.bitmap.iter().map(SymbolOrdinal)
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
