use roaring::RoaringBitmap;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ReferenceOrdinal(u32);

impl ReferenceOrdinal {
	pub(crate) fn from_index(index: usize) -> Self {
		assert!(
			u32::try_from(index).is_ok(),
			"reference index exceeds u32 range"
		);
		Self(index as u32)
	}

	pub(crate) fn index(self) -> usize {
		self.0 as usize
	}

	pub(crate) fn raw(self) -> u32 {
		self.0
	}
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ReferenceSet {
	bitmap: RoaringBitmap,
}

pub(crate) struct ReferenceSetIter<'a>(roaring::bitmap::Iter<'a>);

impl Iterator for ReferenceSetIter<'_> {
	type Item = ReferenceOrdinal;

	fn next(&mut self) -> Option<Self::Item> {
		self.0.next().map(ReferenceOrdinal)
	}
}

impl ReferenceSetIter<'_> {
	pub(crate) fn advance_to(&mut self, reference: ReferenceOrdinal) {
		self.0.advance_to(reference.raw());
	}
}

impl ReferenceSet {
	pub(crate) fn new() -> Self {
		Self {
			bitmap: RoaringBitmap::new(),
		}
	}

	pub(crate) fn contains(&self, reference: ReferenceOrdinal) -> bool {
		self.bitmap.contains(reference.raw())
	}

	pub(crate) fn is_empty(&self) -> bool {
		self.bitmap.is_empty()
	}

	pub(crate) fn len(&self) -> u64 {
		self.bitmap.len()
	}

	pub(crate) fn serialized_size(&self) -> usize {
		self.bitmap.serialized_size()
	}

	pub(crate) fn intersect_with(&mut self, other: &Self) {
		self.bitmap &= &other.bitmap;
	}

	pub(crate) fn union_with(&mut self, other: &Self) {
		self.bitmap |= &other.bitmap;
	}

	pub(crate) fn remove_all(&mut self, stale: &Self) {
		self.bitmap -= &stale.bitmap;
	}

	pub(crate) fn intersection(&self, other: &Self) -> Self {
		let mut result = self.clone();
		result.intersect_with(other);
		result
	}

	pub(crate) fn union(&self, other: &Self) -> Self {
		let mut result = self.clone();
		result.union_with(other);
		result
	}

	pub(crate) fn difference(&self, other: &Self) -> Self {
		let mut result = self.clone();
		result.remove_all(other);
		result
	}

	pub(crate) fn iter(&self) -> impl Iterator<Item = ReferenceOrdinal> + '_ {
		self.bitmap.iter().map(ReferenceOrdinal)
	}

	pub(crate) fn ordered_iter(&self) -> ReferenceSetIter<'_> {
		ReferenceSetIter(self.bitmap.iter())
	}

	pub(crate) fn insert(&mut self, reference: ReferenceOrdinal) -> bool {
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
