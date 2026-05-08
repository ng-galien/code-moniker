use std::collections::HashSet;

use crate::core::moniker::Moniker;

use super::kinds;
use super::walker::{ImportEntry, Walker};

pub(super) fn visibility_from_name(name: &[u8]) -> &'static [u8] {
	match name.first().copied() {
		Some(b) if b.is_ascii_uppercase() => kinds::VIS_PUBLIC,
		_ => kinds::VIS_MODULE,
	}
}

pub(super) fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::FUNCTION || last.kind == kinds::METHOD
}

impl<'src> Walker<'src> {
	pub(super) fn push_local_scope(&self) {
		self.local_scope.borrow_mut().push(HashSet::new());
	}

	pub(super) fn pop_local_scope(&self) {
		self.local_scope.borrow_mut().pop();
	}

	pub(super) fn record_local(&self, name: &'src [u8]) {
		if let Some(top) = self.local_scope.borrow_mut().last_mut() {
			top.insert(name);
		}
	}

	pub(super) fn is_local_name(&self, name: &[u8]) -> bool {
		self.local_scope
			.borrow()
			.iter()
			.any(|frame| frame.contains(name))
	}

	pub(super) fn name_confidence(&self, name: &[u8]) -> Option<&'static [u8]> {
		crate::lang::kinds::name_confidence_for(self.is_local_name(name), self.deep)
	}

	pub(super) fn import_entry_for(&self, name: &[u8]) -> Option<ImportEntry> {
		self.imports.borrow().get(name).cloned()
	}

	pub(super) fn import_confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.imports.borrow().get(name).map(|e| e.confidence)
	}

	pub(super) fn resolve_type_target(
		&self,
		name: &[u8],
		fallback_kind: &[u8],
	) -> (Moniker, &'static [u8]) {
		if let Some(m) = self.type_table.get(name) {
			return (m.clone(), kinds::CONF_RESOLVED);
		}
		let target = super::canonicalize::extend_segment(&self.module, fallback_kind, name);
		let confidence = self
			.import_confidence_for(name)
			.unwrap_or(kinds::CONF_NAME_MATCH);
		(target, confidence)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn capitalized_name_is_public() {
		assert_eq!(visibility_from_name(b"Foo"), kinds::VIS_PUBLIC);
	}

	#[test]
	fn lowercase_name_is_module() {
		assert_eq!(visibility_from_name(b"foo"), kinds::VIS_MODULE);
	}

	#[test]
	fn empty_name_is_module() {
		assert_eq!(visibility_from_name(b""), kinds::VIS_MODULE);
	}

	#[test]
	fn underscore_name_is_module() {
		assert_eq!(visibility_from_name(b"_internal"), kinds::VIS_MODULE);
	}
}
