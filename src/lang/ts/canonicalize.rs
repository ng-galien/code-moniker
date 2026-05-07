//! Build moniker values from tree-sitter nodes and the importing
//! module's anchor.

use tree_sitter::Node;

use crate::core::kind_registry::KindId;
use crate::core::moniker::{Moniker, MonikerBuilder};

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str, path_kind: KindId) -> Moniker {
	let stem = strip_known_extension(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	append_path_segments(&mut builder, stem, path_kind);
	builder.build()
}

/// Split a `/`-separated path and append each non-empty, non-`.` piece
/// as a segment with the given kind. Shared between module-moniker
/// construction and import-target resolution.
pub(super) fn append_path_segments(b: &mut MonikerBuilder, path: &str, kind: KindId) {
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

pub(super) fn extend_segment(parent: &Moniker, kind: KindId, bytes: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, bytes);
	b.build()
}

pub(super) fn extend_method(parent: &Moniker, kind: KindId, bytes: &[u8], arity: u16) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.method(kind, bytes, arity);
	b.build()
}

pub(super) fn node_position(node: Node<'_>) -> (u32, u32) {
	(node.start_byte() as u32, node.end_byte() as u32)
}
