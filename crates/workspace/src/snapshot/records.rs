//! Slot-aligned record storage for the code index.
//!
//! Records live in one immutable shard per catalog file slot, shared via
//! `Arc`. Cloning the table bumps refcounts; refreshing a file replaces one
//! shard. The API mirrors slices (`len`, `get`, `iter`, `Index`) so
//! consumers keep positional record indexes; positions are resolved through
//! the shard offset table.

use std::ops::Index;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct RecordTable<T> {
	shards: Vec<Arc<[T]>>,
	offsets: Vec<usize>,
}

impl<T> RecordTable<T> {
	pub fn from_shards(shards: Vec<Arc<[T]>>) -> Self {
		let mut table = Self {
			shards,
			offsets: Vec::new(),
		};
		table.rebuild_offsets();
		table
	}

	pub fn from_records(records: Vec<T>) -> Self {
		Self::from_shards(vec![Arc::from(records)])
	}

	pub fn len(&self) -> usize {
		self.offsets.last().copied().unwrap_or(0)
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	pub fn get(&self, index: usize) -> Option<&T> {
		if index >= self.len() {
			return None;
		}
		let slot = self.offsets.partition_point(|offset| *offset <= index) - 1;
		self.shards[slot].get(index - self.offsets[slot])
	}

	pub fn iter(&self) -> impl Iterator<Item = &T> + '_ {
		self.shards.iter().flat_map(|shard| shard.iter())
	}

	pub fn file_records(&self, slot: usize) -> &[T] {
		self.shards.get(slot).map(Arc::as_ref).unwrap_or(&[])
	}

	pub(crate) fn replace(&mut self, slot: usize, records: Arc<[T]>) {
		if let Some(shard) = self.shards.get_mut(slot) {
			*shard = records;
		}
		self.rebuild_offsets();
	}

	fn rebuild_offsets(&mut self) {
		self.offsets.clear();
		self.offsets.reserve(self.shards.len() + 1);
		let mut total = 0usize;
		self.offsets.push(0);
		for shard in &self.shards {
			total += shard.len();
			self.offsets.push(total);
		}
	}
}

impl<T> Index<usize> for RecordTable<T> {
	type Output = T;

	fn index(&self, index: usize) -> &T {
		self.get(index)
			.unwrap_or_else(|| panic!("record index {index} out of bounds"))
	}
}

impl<T: PartialEq> PartialEq for RecordTable<T> {
	fn eq(&self, other: &Self) -> bool {
		self.len() == other.len() && self.iter().eq(other.iter())
	}
}

impl<T: Eq> Eq for RecordTable<T> {}

#[cfg(test)]
mod tests {
	use super::*;

	fn table() -> RecordTable<u32> {
		RecordTable::from_shards(vec![
			Arc::from(vec![1u32, 2]),
			Arc::from(Vec::<u32>::new()),
			Arc::from(vec![3u32, 4, 5]),
		])
	}

	#[test]
	fn positions_span_shards_with_empty_slots() {
		let table = table();
		assert_eq!(table.len(), 5);
		assert_eq!(table.get(0), Some(&1));
		assert_eq!(table.get(1), Some(&2));
		assert_eq!(table.get(2), Some(&3));
		assert_eq!(table.get(4), Some(&5));
		assert_eq!(table.get(5), None);
		assert_eq!(
			table.iter().copied().collect::<Vec<_>>(),
			vec![1, 2, 3, 4, 5]
		);
		assert_eq!(table.file_records(1), &[] as &[u32]);
		assert_eq!(table.file_records(9), &[] as &[u32]);
	}

	#[test]
	fn replace_swaps_one_shard_and_reindexes() {
		let mut table = table();
		table.replace(0, Arc::from(vec![9u32]));
		assert_eq!(table.iter().copied().collect::<Vec<_>>(), vec![9, 3, 4, 5]);
		assert_eq!(table[1], 3);
	}

	#[test]
	fn equality_ignores_shard_boundaries() {
		let flat = RecordTable::from_records(vec![1u32, 2, 3, 4, 5]);
		assert_eq!(table(), flat);
	}
}
