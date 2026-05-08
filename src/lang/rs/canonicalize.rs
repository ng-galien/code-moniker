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
	builder.segment(crate::lang::kinds::LANG, b"rs");
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

pub(super) use crate::lang::callable::{extend_callable_typed, extend_segment};

pub(super) fn node_position(node: Node<'_>) -> (u32, u32) {
	(node.start_byte() as u32, node.end_byte() as u32)
}

/// Parameter type list for a `function_item`. Each `parameter` node
/// contributes its `type` field text as written in source (short
/// names, generics and lifetimes preserved). `self_parameter` is
/// implicit (excluded from the value-parameter signature, same as
/// Java's `this`). `variadic_parameter` (FFI `...`) maps to the
/// literal `...` slot text.
pub(super) fn function_param_types<'src>(
	node: Node<'_>,
	source: &'src [u8],
) -> Vec<&'src str> {
	let Some(params) = node.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		match child.kind() {
			"parameter" => {
				let t = child
					.child_by_field_name("type")
					.and_then(|n| n.utf8_text(source).ok())
					.unwrap_or("_");
				out.push(t.trim());
			}
			"variadic_parameter" => out.push("..."),
			"self_parameter" => {} // implicit, excluded
			_ => {}
		}
	}
	out
}

/// Parameter type list for a `closure_expression`. Slot text comes
/// from the `type` field when the parameter is a `parameter` wrapper
/// (`|x: i32|`); bare patterns (`|x|`) get the `_` placeholder.
/// Destructuring patterns count as one slot regardless of depth.
pub(super) fn closure_param_types<'src>(
	closure: Node<'_>,
	source: &'src [u8],
) -> Vec<&'src str> {
	let Some(params) = closure.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		if child.kind() == "parameter" {
			let t = child
				.child_by_field_name("type")
				.and_then(|n| n.utf8_text(source).ok())
				.unwrap_or("_");
			out.push(t.trim());
		} else {
			out.push("_");
		}
	}
	out
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
