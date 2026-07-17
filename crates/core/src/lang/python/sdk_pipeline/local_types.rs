use std::collections::BTreeSet;

use crate::core::moniker::Moniker;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct LocalTypeSet {
	types: BTreeSet<Moniker>,
	dynamic: bool,
}

impl LocalTypeSet {
	pub(super) fn from_type(target: Moniker) -> Self {
		let mut types = BTreeSet::new();
		types.insert(target);
		Self {
			types,
			dynamic: false,
		}
	}

	pub(super) fn insert(&mut self, target: Moniker) {
		self.types.insert(target);
	}

	pub(super) fn union_with(&mut self, other: Self) {
		self.types.extend(other.types);
		self.dynamic |= other.dynamic;
	}

	pub(super) fn mark_dynamic(&mut self) {
		self.dynamic = true;
	}

	pub(super) fn unique(&self) -> Option<Moniker> {
		(!self.dynamic && self.types.len() == 1)
			.then(|| self.types.iter().next().cloned())
			.flatten()
	}

	pub(super) fn static_types(&self) -> impl Iterator<Item = &Moniker> {
		self.types.iter()
	}

	pub(super) fn is_dynamic(&self) -> bool {
		self.dynamic
	}

	pub(super) fn is_empty(&self) -> bool {
		self.types.is_empty()
	}
}
