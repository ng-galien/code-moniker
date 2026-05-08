
use std::collections::HashSet;

use tree_sitter::Node;

use crate::core::moniker::Moniker;

use super::kinds;
use super::walker::Walker;

pub(super) fn collect_export_ranges(root: Node<'_>) -> Vec<(u32, u32)> {
	let mut out = Vec::new();
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() == "export_statement" {
			out.push((child.start_byte() as u32, child.end_byte() as u32));
		}
	}
	out
}

pub(super) fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else { return false };
	last.kind == kinds::FUNCTION || last.kind == kinds::METHOD || last.kind == kinds::CONSTRUCTOR
}

pub(super) fn class_member_visibility(node: Node<'_>, source: &[u8]) -> &'static [u8] {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "accessibility_modifier" {
			return match c.utf8_text(source).unwrap_or("") {
				"private" => kinds::VIS_PRIVATE,
				"protected" => kinds::VIS_PROTECTED,
				_ => kinds::VIS_PUBLIC,
			};
		}
	}
	kinds::VIS_PUBLIC
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

pub(super) fn collect_binding_names<'src>(
	pat: Node<'_>,
	source: &'src [u8],
) -> Vec<&'src str> {
	fn rec<'src>(node: Node<'_>, source: &'src [u8], out: &mut Vec<&'src str>) {
		match node.kind() {
			"identifier" | "shorthand_property_identifier_pattern" => {
				if let Ok(s) = node.utf8_text(source) {
					out.push(s);
				}
			}
			"object_pattern" | "array_pattern" | "pair_pattern" | "rest_pattern"
			| "assignment_pattern" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					rec(c, source, out);
				}
			}
			_ => {}
		}
	}
	let mut out = Vec::new();
	rec(pat, source, &mut out);
	out
}

impl<'src> Walker<'src> {
	pub(super) fn is_exported(&self, node: Node<'_>) -> bool {
		let s = node.start_byte() as u32;
		self.export_ranges.iter().any(|(a, b)| *a <= s && s < *b)
	}

	pub(super) fn module_visibility(&self, node: Node<'_>) -> &'static [u8] {
		if self.is_exported(node) {
			kinds::VIS_PUBLIC
		} else {
			kinds::VIS_MODULE
		}
	}

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
		if self.is_local_name(name) {
			if self.deep {
				Some(kinds::CONF_LOCAL)
			} else {
				None
			}
		} else {
			Some(kinds::CONF_NAME_MATCH)
		}
	}
}
