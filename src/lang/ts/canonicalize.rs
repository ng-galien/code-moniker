//! Build moniker values from tree-sitter nodes and the importing
//! module's anchor.

use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

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
	// `.d.ts` is listed first so it wins over `.ts`.
	const EXTS: &[&str] = &[
		".d.ts", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".mts", ".cts",
	];
	EXTS.iter()
		.find_map(|ext| uri.strip_suffix(ext))
		.unwrap_or(uri)
}

pub(super) fn extend_segment(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, name);
	b.build()
}

/// Build a callable moniker. Arity 0 → `bar()`, arity N → `bar(N)`.
/// Disambiguator lives in the segment name; v2 has no separate arity
/// field.
pub(super) fn extend_method(parent: &Moniker, kind: &[u8], name: &[u8], arity: u16) -> Moniker {
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

/// Anonymous-callback name keyed on the AST node's start position. Both
/// row and column are 0-based as exposed by tree-sitter.
pub(super) fn anonymous_callback_name(node: Node<'_>) -> Vec<u8> {
	let p = node.start_position();
	format!("__cb_{}_{}", p.row, p.column).into_bytes()
}

/// Count parameters of a `function`/`method`/`arrow_function`/`function_expression`
/// node. The `parameter` field shape covers the arrow `x => …` form.
pub(super) fn callable_arity(node: Node<'_>) -> u16 {
	if let Some(params) = node.child_by_field_name("parameters") {
		let mut cursor = params.walk();
		let mut count: u16 = 0;
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"required_parameter" | "optional_parameter" => count += 1,
				// rest pattern at the end of a TS formal_parameters list
				"rest_pattern" => count += 1,
				_ => {}
			}
		}
		return count;
	}
	if node.child_by_field_name("parameter").is_some() {
		return 1;
	}
	0
}

pub(super) fn external_pkg_builder(project: &[u8], pkg: &str) -> MonikerBuilder {
	let (head, tail) = split_package_specifier(pkg);
	let mut b = MonikerBuilder::new();
	b.project(project);
	b.segment(kinds::EXTERNAL_PKG, head.as_bytes());
	for piece in tail.split('/').filter(|s| !s.is_empty()) {
		b.segment(kinds::PATH, piece.as_bytes());
	}
	b
}

/// Split `lodash/fp/get` → (`lodash`, `fp/get`); `@scope/pkg/sub` →
/// (`@scope/pkg`, `sub`); `react` → (`react`, ``).
fn split_package_specifier(spec: &str) -> (&str, &str) {
	if spec.starts_with('@') {
		let after_scope = &spec[1..];
		let mut parts = after_scope.splitn(3, '/');
		let scope = parts.next().unwrap_or("");
		let name = parts.next().unwrap_or("");
		let tail = parts.next().unwrap_or("");
		let head_end = if name.is_empty() {
			1 + scope.len()
		} else {
			1 + scope.len() + 1 + name.len()
		};
		(&spec[..head_end], tail)
	} else {
		match spec.find('/') {
			Some(i) => (&spec[..i], &spec[i + 1..]),
			None => (spec, ""),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn split_specifier_simple_keeps_pkg() {
		assert_eq!(split_package_specifier("react"), ("react", ""));
	}

	#[test]
	fn split_specifier_with_subpath() {
		assert_eq!(
			split_package_specifier("lodash/fp/get"),
			("lodash", "fp/get")
		);
	}

	#[test]
	fn split_specifier_scoped_keeps_full_head() {
		assert_eq!(split_package_specifier("@scope/pkg"), ("@scope/pkg", ""));
	}

	#[test]
	fn split_specifier_scoped_with_subpath() {
		assert_eq!(
			split_package_specifier("@scope/pkg/sub/path"),
			("@scope/pkg", "sub/path")
		);
	}

	#[test]
	fn callable_segment_name_arity_zero_drops_number() {
		assert_eq!(callable_segment_name(b"bar", 0), b"bar()".to_vec());
	}

	#[test]
	fn callable_segment_name_keeps_arity_number() {
		assert_eq!(callable_segment_name(b"bar", 3), b"bar(3)".to_vec());
	}

	#[test]
	fn strip_known_extension_handles_d_ts_first() {
		assert_eq!(strip_known_extension("types.d.ts"), "types");
		assert_eq!(strip_known_extension("util.ts"), "util");
		assert_eq!(strip_known_extension("util.cjs"), "util");
		assert_eq!(strip_known_extension("util"), "util");
	}
}
