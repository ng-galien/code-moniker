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

pub(super) use crate::lang::callable::{
	extend_callable_arity, extend_callable_typed, extend_segment,
};

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

}
