use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{SymbolId, SymbolOrdinal, SymbolSet};

// An ordinal catalogue is a bidirectional index; its lookup and lifecycle
// methods intentionally touch different sides of the same invariant.
// code-moniker: ignore[smell-god-type-local-metrics]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SymbolOrdinalCatalog {
	next_ordinal: u32,
	ids: FxHashMap<SymbolOrdinal, SymbolId>,
	identities: FxHashMap<SymbolOrdinal, Arc<str>>,
	ordinals_by_id: FxHashMap<SymbolId, SymbolOrdinal>,
	ordinals_by_identity: FxHashMap<Arc<str>, SymbolOrdinal>,
}

impl SymbolOrdinalCatalog {
	pub(super) fn estimated_heap_bytes(&self) -> usize {
		self.ids.capacity()
			* (std::mem::size_of::<SymbolOrdinal>() + std::mem::size_of::<SymbolId>())
			+ self.identities.capacity()
				* (std::mem::size_of::<SymbolOrdinal>() + std::mem::size_of::<Arc<str>>())
			+ self.ordinals_by_id.capacity()
				* (std::mem::size_of::<SymbolId>() + std::mem::size_of::<SymbolOrdinal>())
			+ self.ordinals_by_identity.capacity()
				* (std::mem::size_of::<Arc<str>>() + std::mem::size_of::<SymbolOrdinal>())
	}

	pub fn push(&mut self, id: SymbolId, identity: Arc<str>) -> SymbolOrdinal {
		if let Some(ordinal) = self.ordinals_by_identity.get(&identity).copied() {
			self.rebind_id(ordinal, id);
			return ordinal;
		}
		let ordinal = SymbolOrdinal::from_index(self.next_ordinal as usize);
		assert!(
			self.next_ordinal < u32::MAX,
			"symbol ordinal space exhausted"
		);
		self.next_ordinal += 1;
		self.ordinals_by_id.insert(id, ordinal);
		self.ordinals_by_identity
			.insert(Arc::clone(&identity), ordinal);
		self.ids.insert(ordinal, id);
		self.identities.insert(ordinal, identity);
		ordinal
	}

	fn rebind_id(&mut self, ordinal: SymbolOrdinal, id: SymbolId) {
		if let Some(previous_id) = self.ids.get(&ordinal) {
			if previous_id == &id {
				return;
			}
			if self.ordinals_by_id.get(previous_id) == Some(&ordinal) {
				self.ordinals_by_id.remove(previous_id);
			}
		}
		self.ordinals_by_id.insert(id, ordinal);
		self.ids.insert(ordinal, id);
	}

	pub fn retire(&mut self, ordinal: SymbolOrdinal) {
		self.unbind_id(ordinal);
		if let Some(identity) = self.identities.remove(&ordinal) {
			self.ordinals_by_identity.remove(&identity);
		}
	}

	pub fn unbind_id(&mut self, ordinal: SymbolOrdinal) {
		if let Some(previous_id) = self.ids.remove(&ordinal)
			&& self.ordinals_by_id.get(&previous_id) == Some(&ordinal)
		{
			self.ordinals_by_id.remove(&previous_id);
		}
	}

	pub fn identity(&self, ordinal: SymbolOrdinal) -> Option<&str> {
		self.identities.get(&ordinal).map(AsRef::as_ref)
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

	pub fn ordinal_by_identity(&self, identity: &str) -> Option<SymbolOrdinal> {
		self.ordinals_by_identity.get(identity).copied()
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
