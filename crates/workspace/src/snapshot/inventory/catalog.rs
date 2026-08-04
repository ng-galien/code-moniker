use rustc_hash::FxHashMap;

use super::{SymbolId, SymbolOrdinal, SymbolSet};

// An ordinal catalogue is a bidirectional index; its lookup and lifecycle
// methods intentionally touch different sides of the same invariant.
// code-moniker: ignore[smell-god-type-local-metrics]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SymbolOrdinalCatalog {
	next_ordinal: u32,
	ids: FxHashMap<SymbolOrdinal, SymbolId>,
	ordinals_by_id: FxHashMap<SymbolId, SymbolOrdinal>,
}

impl SymbolOrdinalCatalog {
	pub(super) fn estimated_heap_bytes(&self) -> usize {
		self.ids.capacity()
			* (std::mem::size_of::<SymbolOrdinal>() + std::mem::size_of::<SymbolId>())
			+ self.ordinals_by_id.capacity()
				* (std::mem::size_of::<SymbolId>() + std::mem::size_of::<SymbolOrdinal>())
	}

	pub fn push(&mut self, id: SymbolId) -> SymbolOrdinal {
		if let Some(ordinal) = self.ordinals_by_id.get(&id).copied() {
			return ordinal;
		}
		let ordinal = SymbolOrdinal::from_index(self.next_ordinal as usize);
		assert!(
			self.next_ordinal < u32::MAX,
			"symbol ordinal space exhausted"
		);
		self.next_ordinal += 1;
		self.bind_id(ordinal, id);
		ordinal
	}

	pub fn retire(&mut self, ordinal: SymbolOrdinal) {
		self.unbind_id(ordinal);
	}

	pub(super) fn bind_id(&mut self, ordinal: SymbolOrdinal, id: SymbolId) {
		if let Some(previous) = self.ids.insert(ordinal, id) {
			self.ordinals_by_id.remove(&previous);
		}
		if let Some(previous_ordinal) = self.ordinals_by_id.insert(id, ordinal)
			&& previous_ordinal != ordinal
		{
			self.ids.remove(&previous_ordinal);
		}
	}

	pub fn unbind_id(&mut self, ordinal: SymbolOrdinal) {
		if let Some(previous_id) = self.ids.remove(&ordinal)
			&& self.ordinals_by_id.get(&previous_id) == Some(&ordinal)
		{
			self.ordinals_by_id.remove(&previous_id);
		}
	}

	pub fn len(&self) -> usize {
		self.ordinals_by_id.len()
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	pub fn storage_len(&self) -> usize {
		self.ids.len()
	}

	pub fn ordinal(&self, id: &SymbolId) -> Option<SymbolOrdinal> {
		self.ordinals_by_id.get(id).copied()
	}

	pub fn ids(&self, symbols: &SymbolSet) -> Vec<SymbolId> {
		symbols
			.iter()
			.filter_map(|symbol| self.id(symbol).copied())
			.collect()
	}

	pub fn id(&self, ordinal: SymbolOrdinal) -> Option<&SymbolId> {
		self.ids.get(&ordinal)
	}

	pub fn active_ordinals(&self) -> impl Iterator<Item = (u32, SymbolId)> + '_ {
		let mut ordinals = self
			.ids
			.iter()
			.map(|(ordinal, id)| (ordinal.raw(), *id))
			.collect::<Vec<_>>();
		ordinals.sort_unstable_by_key(|(ordinal, _)| *ordinal);
		ordinals.into_iter()
	}
}
