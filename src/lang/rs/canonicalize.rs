//! Build moniker values from tree-sitter-rust nodes and the importing
//! module's anchor. Centralized per the canonicalization contract — no
//! moniker construction inlined in walker.rs / refs.rs.

use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

/// Compute the file-as-module moniker by walking the URI's directory
/// chain under `anchor`, emitting `path:<dir>` segments for each
/// directory and a final `module:<basename>` for the file stem.
pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let stem = strip_rs_extension(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	let pieces: Vec<&str> = stem.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
	if pieces.is_empty() {
		return builder.build();
	}
	let last = pieces.len() - 1;
	for (i, piece) in pieces.iter().enumerate() {
		let kind = if i == last { kinds::MODULE } else { kinds::PATH };
		builder.segment(kind, piece.as_bytes());
	}
	builder.build()
}

pub(super) fn strip_rs_extension(uri: &str) -> &str {
	uri.strip_suffix(".rs").unwrap_or(uri)
}

pub(super) fn extend(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, name);
	b.build()
}

/// Build a method moniker. Arity 0 → name `bar()`, arity N → `bar(N)`.
/// Disambiguator lives in the segment name; v2 has no separate arity field.
pub(super) fn extend_method(parent: &Moniker, kind: &[u8], name: &[u8], arity: u16) -> Moniker {
	let mut full = Vec::with_capacity(name.len() + 6);
	full.extend_from_slice(name);
	full.push(b'(');
	if arity != 0 {
		full.extend_from_slice(arity.to_string().as_bytes());
	}
	full.push(b')');
	extend(parent, kind, &full)
}

pub(super) fn node_position(node: Node<'_>) -> (u32, u32) {
	(node.start_byte() as u32, node.end_byte() as u32)
}

/// Count parameters of a `function_item` body. Returns 0 when the
/// function has no `(...)` or it parses as empty. The disambiguator
/// goes into the method name; static signatures (param types) are a
/// later concern that requires extending DefRecord.
pub(super) fn function_arity(node: Node<'_>, source: &[u8]) -> u16 {
	let Some(params) = node.child_by_field_name("parameters") else {
		return 0;
	};
	let mut cursor = params.walk();
	let mut count: u16 = 0;
	for child in params.named_children(&mut cursor) {
		match child.kind() {
			"parameter" | "self_parameter" | "variadic_parameter" => count += 1,
			_ => {}
		}
	}
	let _ = source;
	count
}

/// Bare type name for a `type` field of an `impl_item`. Strips generic
/// arguments (`Foo<T>` → `Foo`) and qualified paths (keeps the last
/// `::`-separated component) so the moniker maps to the in-module type.
pub(super) fn impl_type_name<'a>(node: Node<'a>, source: &'a [u8]) -> Option<&'a str> {
	let target = match node.kind() {
		"generic_type" => node.child_by_field_name("type")?,
		_ => node,
	};
	match target.kind() {
		"type_identifier" | "primitive_type" => target.utf8_text(source).ok(),
		"scoped_type_identifier" => target
			.child_by_field_name("name")
			.and_then(|n| n.utf8_text(source).ok()),
		_ => target.utf8_text(source).ok(),
	}
}
