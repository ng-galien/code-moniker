
use std::collections::HashSet;

use tree_sitter::Node;

use crate::core::moniker::Moniker;

use super::kinds;
use super::walker::Walker;

pub(super) fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else { return false };
	last.kind == kinds::METHOD || last.kind == kinds::CONSTRUCTOR
}

pub(super) fn modifier_visibility(node: Node<'_>) -> &'static [u8] {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() != "modifiers" {
			continue;
		}
		let mut mc = child.walk();
		for m in child.children(&mut mc) {
			match m.kind() {
				"public" => return kinds::VIS_PUBLIC,
				"protected" => return kinds::VIS_PROTECTED,
				"private" => return kinds::VIS_PRIVATE,
				_ => {}
			}
		}
	}
	kinds::VIS_PACKAGE
}

pub(super) fn section_title<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
	let raw = node.utf8_text(source).ok()?;
	let body = raw
		.strip_prefix("//")
		.or_else(|| raw.strip_prefix("/*").and_then(|s| s.strip_suffix("*/")))
		.unwrap_or(raw);
	let body = body.trim();
	let stripped = body.trim_matches(|c: char| c == '=' || c == '-' || c.is_whitespace());
	if stripped.is_empty() {
		return None;
	}
	let starts = body.starts_with("==") || body.starts_with("--");
	let ends = body.ends_with("==") || body.ends_with("--");
	(starts && ends).then_some(stripped)
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

	pub(super) fn name_confidence(&self, name: &[u8]) -> &'static [u8] {
		if self.is_local_name(name) {
			kinds::CONF_LOCAL
		} else {
			kinds::CONF_NAME_MATCH
		}
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
