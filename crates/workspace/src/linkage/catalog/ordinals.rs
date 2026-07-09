use std::sync::Arc;

use roaring::RoaringBitmap;
use rustc_hash::FxHashMap;

use crate::snapshot::SymbolId;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::linkage) struct ReferenceOrdinal(u32);

impl ReferenceOrdinal {
	pub(in crate::linkage) fn from_index(index: usize) -> Self {
		assert!(
			u32::try_from(index).is_ok(),
			"reference index exceeds u32 range"
		);
		Self(index as u32)
	}

	pub(in crate::linkage) fn index(self) -> usize {
		self.0 as usize
	}

	pub(in crate::linkage) fn raw(self) -> u32 {
		self.0
	}
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::linkage) struct SymbolOrdinal(u32);

impl SymbolOrdinal {
	pub(in crate::linkage) fn from_index(index: usize) -> Self {
		assert!(
			u32::try_from(index).is_ok(),
			"symbol index exceeds u32 range"
		);
		Self(index as u32)
	}

	pub(in crate::linkage) fn index(self) -> usize {
		self.0 as usize
	}

	pub(in crate::linkage) fn raw(self) -> u32 {
		self.0
	}
}

#[derive(Clone, Debug, Default)]
pub(in crate::linkage) struct ReferenceSet {
	bitmap: RoaringBitmap,
}

impl ReferenceSet {
	pub(in crate::linkage) fn new() -> Self {
		Self {
			bitmap: RoaringBitmap::new(),
		}
	}

	pub(in crate::linkage) fn insert(&mut self, reference: ReferenceOrdinal) -> bool {
		self.bitmap.insert(reference.raw())
	}

	pub(in crate::linkage) fn contains(&self, reference: ReferenceOrdinal) -> bool {
		self.bitmap.contains(reference.raw())
	}

	pub(in crate::linkage) fn is_empty(&self) -> bool {
		self.bitmap.is_empty()
	}

	pub(in crate::linkage) fn len(&self) -> u64 {
		self.bitmap.len()
	}

	pub(in crate::linkage) fn serialized_size(&self) -> usize {
		self.bitmap.serialized_size()
	}

	pub(in crate::linkage) fn union_with(&mut self, other: &Self) {
		self.bitmap |= &other.bitmap;
	}

	pub(in crate::linkage) fn remove_all(&mut self, stale: &Self) {
		self.bitmap -= &stale.bitmap;
	}

	pub(in crate::linkage) fn iter(&self) -> impl Iterator<Item = ReferenceOrdinal> + '_ {
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
pub(in crate::linkage) struct SymbolSet {
	bitmap: RoaringBitmap,
}

impl SymbolSet {
	pub(in crate::linkage) fn new() -> Self {
		Self {
			bitmap: RoaringBitmap::new(),
		}
	}

	pub(in crate::linkage) fn insert(&mut self, symbol: SymbolOrdinal) -> bool {
		self.bitmap.insert(symbol.raw())
	}

	pub(in crate::linkage) fn remove(&mut self, symbol: SymbolOrdinal) -> bool {
		self.bitmap.remove(symbol.raw())
	}

	pub(in crate::linkage) fn from_symbol(symbol: SymbolOrdinal) -> Self {
		let mut set = Self::new();
		set.insert(symbol);
		set
	}

	pub(in crate::linkage) fn is_empty(&self) -> bool {
		self.bitmap.is_empty()
	}

	pub(in crate::linkage) fn serialized_size(&self) -> usize {
		self.bitmap.serialized_size()
	}

	pub(in crate::linkage) fn single(&self) -> Option<SymbolOrdinal> {
		(self.len() == 1).then(|| self.iter().next()).flatten()
	}

	pub(in crate::linkage) fn len(&self) -> usize {
		let len = self.bitmap.len();
		assert!(
			usize::try_from(len).is_ok(),
			"symbol set length exceeds usize"
		);
		len as usize
	}

	pub(in crate::linkage) fn iter(&self) -> impl Iterator<Item = SymbolOrdinal> + '_ {
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

#[derive(Clone, Debug, Default)]
pub(in crate::linkage) struct SymbolOrdinalCatalog {
	ids: Vec<Option<SymbolId>>,
	identities: Vec<Option<Arc<str>>>,
	ordinals_by_id: FxHashMap<SymbolId, SymbolOrdinal>,
	ordinals_by_identity: FxHashMap<Arc<str>, SymbolOrdinal>,
}

impl SymbolOrdinalCatalog {
	pub(in crate::linkage) fn push(&mut self, id: SymbolId, identity: Arc<str>) -> SymbolOrdinal {
		if let Some(ordinal) = self.ordinals_by_identity.get(&identity).copied() {
			self.rebind_id(ordinal, id);
			return ordinal;
		}
		let ordinal = SymbolOrdinal::from_index(self.ids.len());
		self.ordinals_by_id.insert(id, ordinal);
		self.ordinals_by_identity
			.insert(Arc::clone(&identity), ordinal);
		self.ids.push(Some(id));
		self.identities.push(Some(identity));
		ordinal
	}

	fn rebind_id(&mut self, ordinal: SymbolOrdinal, id: SymbolId) {
		if let Some(Some(previous_id)) = self.ids.get(ordinal.index()) {
			if previous_id == &id {
				return;
			}
			if self.ordinals_by_id.get(previous_id) == Some(&ordinal) {
				self.ordinals_by_id.remove(&previous_id.clone());
			}
		}
		self.ordinals_by_id.insert(id, ordinal);
		self.ids[ordinal.index()] = Some(id);
	}

	pub(in crate::linkage) fn unbind_id(&mut self, ordinal: SymbolOrdinal) {
		if let Some(slot) = self.ids.get_mut(ordinal.index())
			&& let Some(previous_id) = slot.take()
			&& self.ordinals_by_id.get(&previous_id) == Some(&ordinal)
		{
			self.ordinals_by_id.remove(&previous_id);
		}
	}

	pub(in crate::linkage) fn retire(&mut self, ordinal: SymbolOrdinal) {
		self.unbind_id(ordinal);
		if let Some(slot) = self.identities.get_mut(ordinal.index())
			&& let Some(identity) = slot.take()
		{
			self.ordinals_by_identity.remove(&identity);
		}
	}

	pub(in crate::linkage) fn identity(&self, ordinal: SymbolOrdinal) -> Option<&str> {
		self.identities
			.get(ordinal.index())
			.and_then(|slot| slot.as_deref())
	}

	pub(in crate::linkage) fn len(&self) -> usize {
		self.ordinals_by_identity.len()
	}

	pub(in crate::linkage) fn ordinal(&self, id: &SymbolId) -> Option<SymbolOrdinal> {
		self.ordinals_by_id.get(id).copied()
	}

	pub(in crate::linkage) fn ordinal_by_identity(&self, identity: &str) -> Option<SymbolOrdinal> {
		self.ordinals_by_identity.get(identity).copied()
	}

	pub(in crate::linkage) fn ids(&self, symbols: &SymbolSet) -> Vec<SymbolId> {
		symbols
			.iter()
			.filter_map(|symbol| self.id(symbol).cloned())
			.collect()
	}

	pub(in crate::linkage) fn id(&self, ordinal: SymbolOrdinal) -> Option<&SymbolId> {
		self.ids.get(ordinal.index()).and_then(|slot| slot.as_ref())
	}
}
