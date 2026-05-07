//! Build moniker values from tree-sitter nodes and the importing
//! module's anchor.

use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str, path_kind: &[u8]) -> Moniker {
	let stem = strip_known_extension(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	append_path_segments(&mut builder, stem, path_kind);
	builder.build()
}

pub(super) fn append_path_segments(b: &mut MonikerBuilder, path: &str, kind: &[u8]) {
	for piece in path.split('/').filter(|s| !s.is_empty() && *s != ".") {
		b.segment(kind, piece.as_bytes());
	}
}

pub(super) fn strip_known_extension(uri: &str) -> &str {
	const EXTS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];
	EXTS.iter()
		.find_map(|ext| uri.strip_suffix(ext))
		.unwrap_or(uri)
}

pub(super) fn extend_segment(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, name);
	b.build()
}

/// Build a method moniker. Arity 0 → name `bar()`, arity N → `bar(N)`.
/// Disambiguator lives in the segment name; v2 has no separate arity
/// field.
pub(super) fn extend_method(parent: &Moniker, kind: &[u8], name: &[u8], arity: u16) -> Moniker {
	let mut full = Vec::with_capacity(name.len() + 6);
	full.extend_from_slice(name);
	full.push(b'(');
	if arity != 0 {
		full.extend_from_slice(arity.to_string().as_bytes());
	}
	full.push(b')');
	extend_segment(parent, kind, &full)
}

pub(super) fn node_position(node: Node<'_>) -> (u32, u32) {
	(node.start_byte() as u32, node.end_byte() as u32)
}
