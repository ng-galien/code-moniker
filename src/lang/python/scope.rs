
use std::collections::HashSet;

use crate::core::moniker::Moniker;

use super::kinds;
use super::walker::Walker;

pub(super) fn visibility_from_name(name: &[u8]) -> &'static [u8] {
	if name.len() >= 4 && name.starts_with(b"__") && name.ends_with(b"__") {
		return kinds::VIS_PUBLIC;
	}
	if name.starts_with(b"__") {
		return kinds::VIS_PRIVATE;
	}
	if name.starts_with(b"_") {
		return kinds::VIS_MODULE;
	}
	kinds::VIS_PUBLIC
}

pub(super) fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else { return false };
	last.kind == kinds::FUNCTION || last.kind == kinds::METHOD
}

pub(super) fn is_class_scope(scope: &Moniker) -> bool {
	let Some(last) = scope.as_view().segments().last() else { return false };
	last.kind == kinds::CLASS
}

pub(super) fn section_title<'a>(text: &'a str) -> Option<&'a str> {
	let body = text.strip_prefix('#').unwrap_or(text).trim();
	let starts = body.starts_with("==") || body.starts_with("--");
	let ends = body.ends_with("==") || body.ends_with("--");
	if !(starts && ends) {
		return None;
	}
	let stripped = body.trim_matches(|c: char| c == '=' || c == '-' || c.is_whitespace());
	if stripped.is_empty() {
		None
	} else {
		Some(stripped)
	}
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

	pub(super) fn import_confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.imports.borrow().get(name).copied()
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
	fn dunder_is_public() {
		assert_eq!(visibility_from_name(b"__init__"), kinds::VIS_PUBLIC);
	}

	#[test]
	fn double_underscore_prefix_is_private() {
		assert_eq!(visibility_from_name(b"__secret"), kinds::VIS_PRIVATE);
	}

	#[test]
	fn single_underscore_prefix_is_module() {
		assert_eq!(visibility_from_name(b"_internal"), kinds::VIS_MODULE);
	}

	#[test]
	fn plain_name_is_public() {
		assert_eq!(visibility_from_name(b"foo"), kinds::VIS_PUBLIC);
	}
}
