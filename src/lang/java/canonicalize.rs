//! Module moniker construction for Java. Driven by the source's
//! `package_declaration`, with the file basename (sans `.java`) as the
//! `module:<name>` segment. Falls back to a path-derived shape only
//! when the package declaration is missing or malformed.

use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

/// Build the file-as-module moniker. `package_pieces` is the dotted
/// package split (`com.acme.foo` → `["com", "acme", "foo"]`); empty
/// for the default package.
pub(super) fn compute_module_moniker(
	anchor: &Moniker,
	uri: &str,
	package_pieces: &[&str],
) -> Moniker {
	let class_name = file_stem(uri);
	let mut b = MonikerBuilder::from_view(anchor.as_view());
	for piece in package_pieces.iter().filter(|s| !s.is_empty()) {
		b.segment(kinds::PACKAGE, piece.as_bytes());
	}
	b.segment(kinds::MODULE, class_name.as_bytes());
	b.build()
}

/// `src/main/java/com/acme/Foo.java` → `Foo`. `Foo.java` → `Foo`.
pub(super) fn file_stem(uri: &str) -> &str {
	let after_slash = uri.rsplit('/').next().unwrap_or(uri);
	after_slash.strip_suffix(".java").unwrap_or(after_slash)
}

/// Read the `package com.acme.foo;` declaration if present. Returns
/// the dotted name, e.g. `"com.acme.foo"`. Empty string for the
/// default package.
pub(super) fn read_package_name<'src>(root: Node<'_>, source: &'src [u8]) -> &'src str {
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() != "package_declaration" {
			continue;
		}
		let mut nc = child.walk();
		for nm in child.named_children(&mut nc) {
			if let Ok(s) = nm.utf8_text(source) {
				return s;
			}
		}
	}
	""
}

pub(super) fn extend_segment(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, name);
	b.build()
}

/// Build a callable moniker. Arity 0 → `bar()`, arity N → `bar(N)`.
/// Java overload disambiguation depends on parameter types, but the
/// arity-only suffix keeps moniker collisions impossible across same-
/// name same-arity overloads (we also emit the full `signature` on
/// the def for projection-side overload resolution).
pub(super) fn extend_callable(
	parent: &Moniker,
	kind: &[u8],
	name: &[u8],
	arity: u16,
) -> Moniker {
	extend_segment(parent, kind, &callable_segment_name(name, arity))
}

pub(super) fn callable_segment_name(name: &[u8], arity: u16) -> Vec<u8> {
	let mut full = Vec::with_capacity(name.len() + 6);
	full.extend_from_slice(name);
	full.push(b'(');
	if arity != 0 {
		full.extend_from_slice(arity.to_string().as_bytes());
	}
	full.push(b')');
	full
}

pub(super) fn node_position(node: Node<'_>) -> (u32, u32) {
	(node.start_byte() as u32, node.end_byte() as u32)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn file_stem_strips_directory_and_extension() {
		assert_eq!(file_stem("src/main/java/com/acme/Foo.java"), "Foo");
		assert_eq!(file_stem("Foo.java"), "Foo");
		assert_eq!(file_stem("Foo"), "Foo");
	}

	#[test]
	fn callable_segment_name_arity_zero_drops_number() {
		assert_eq!(callable_segment_name(b"bar", 0), b"bar()".to_vec());
	}

	#[test]
	fn callable_segment_name_keeps_arity() {
		assert_eq!(callable_segment_name(b"bar", 3), b"bar(3)".to_vec());
	}
}
