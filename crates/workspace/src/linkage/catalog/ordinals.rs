use roaring::RoaringBitmap;

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

	pub(in crate::linkage) fn insert(&mut self, reference: ReferenceOrdinal) -> bool {
		self.bitmap.insert(reference.raw())
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
