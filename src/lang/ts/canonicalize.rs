
use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let stem = strip_known_extension(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(crate::lang::kinds::LANG, b"ts");
	append_module_segments(&mut builder, stem);
	builder.build()
}

pub(super) fn append_module_segments(b: &mut MonikerBuilder, path: &str) {
	crate::lang::callable::append_dir_module_segments(b, path, kinds::DIR, kinds::MODULE);
}

pub(super) fn strip_known_extension(uri: &str) -> &str {
	const EXTS: &[&str] = &[
		".d.ts", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".mts", ".cts",
	];
	EXTS.iter()
		.find_map(|ext| uri.strip_suffix(ext))
		.unwrap_or(uri)
}

pub(super) use crate::lang::callable::{
	extend_callable_arity, extend_callable_typed, extend_segment,
};

pub(super) fn node_position(node: Node<'_>) -> (u32, u32) {
	(node.start_byte() as u32, node.end_byte() as u32)
}

pub(super) fn anonymous_callback_name(node: Node<'_>) -> Vec<u8> {
	let p = node.start_position();
	format!("__cb_{}_{}", p.row, p.column).into_bytes()
}

pub(super) fn callable_param_types(node: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	if let Some(params) = node.child_by_field_name("parameters") {
		let mut out = Vec::new();
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"required_parameter" | "optional_parameter" => {
					out.push(parameter_type_text(child, source));
				}
				"rest_pattern" => out.push(b"_".to_vec()),
				_ => {}
			}
		}
		return out;
	}
	if node.child_by_field_name("parameter").is_some() {
		return vec![b"_".to_vec()];
	}
	Vec::new()
}

fn parameter_type_text(param: Node<'_>, source: &[u8]) -> Vec<u8> {
	let Some(annot) = param.child_by_field_name("type") else { return b"_".to_vec() };
	let inner = annot
		.named_child(0)
		.unwrap_or(annot);
	let text = inner.utf8_text(source).unwrap_or("_");
	crate::lang::callable::normalize_type_text(text)
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
	fn strip_known_extension_handles_d_ts_first() {
		assert_eq!(strip_known_extension("types.d.ts"), "types");
		assert_eq!(strip_known_extension("util.ts"), "util");
		assert_eq!(strip_known_extension("util.cjs"), "util");
		assert_eq!(strip_known_extension("util"), "util");
	}
}
