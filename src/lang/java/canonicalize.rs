use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

pub(super) fn compute_module_moniker(
	anchor: &Moniker,
	uri: &str,
	package_pieces: &[&str],
) -> Moniker {
	let class_name = file_stem(uri);
	let mut b = MonikerBuilder::from_view(anchor.as_view());
	b.segment(crate::lang::kinds::LANG, b"java");
	for piece in package_pieces.iter().filter(|s| !s.is_empty()) {
		b.segment(kinds::PACKAGE, piece.as_bytes());
	}
	b.segment(kinds::MODULE, class_name.as_bytes());
	b.build()
}

pub(super) fn file_stem(uri: &str) -> &str {
	let after_slash = uri.rsplit('/').next().unwrap_or(uri);
	after_slash.strip_suffix(".java").unwrap_or(after_slash)
}

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

pub(super) use crate::lang::callable::extend_callable_typed;

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
