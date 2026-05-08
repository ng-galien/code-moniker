//! Python module-moniker construction. The URI path under the anchor
//! is the package chain — there is no in-source `package` declaration
//! to parse.

use std::path::Path;

use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let (pkg_pieces, module_name) = split_path(uri);
	let mut b = MonikerBuilder::from_view(anchor.as_view());
	b.segment(crate::lang::kinds::LANG, b"python");
	for piece in pkg_pieces {
		b.segment(kinds::PACKAGE, piece.as_bytes());
	}
	b.segment(kinds::MODULE, module_name.as_bytes());
	b.build()
}

/// `pkg/sub/foo.py` → (`["pkg", "sub"]`, `"foo"`). `foo.py` → (`[]`,
/// `"foo"`). `__init__.py` keeps the literal name `__init__`.
fn split_path<'a>(uri: &'a str) -> (Vec<&'a str>, &'a str) {
	let after_scheme = uri.split("://").last().unwrap_or(uri);
	let pieces: Vec<&str> = after_scheme.split('/').filter(|s| !s.is_empty()).collect();
	if pieces.is_empty() {
		return (Vec::new(), "");
	}
	let (last, head) = pieces.split_last().expect("non-empty");
	(head.to_vec(), strip_py_suffix(last))
}

fn strip_py_suffix(name: &str) -> &str {
	Path::new(name)
		.file_stem()
		.and_then(|s| s.to_str())
		.unwrap_or(name)
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
	fn split_path_strips_extension() {
		let (pkg, name) = split_path("foo.py");
		assert!(pkg.is_empty());
		assert_eq!(name, "foo");
	}

	#[test]
	fn split_path_keeps_init_module_name() {
		let (pkg, name) = split_path("acme/__init__.py");
		assert_eq!(pkg, vec!["acme"]);
		assert_eq!(name, "__init__");
	}

	#[test]
	fn split_path_emits_package_chain() {
		let (pkg, name) = split_path("acme/util/text.py");
		assert_eq!(pkg, vec!["acme", "util"]);
		assert_eq!(name, "text");
	}
}
