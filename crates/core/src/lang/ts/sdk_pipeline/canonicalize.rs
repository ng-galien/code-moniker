use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::super::kinds;

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	module_builder_for_path(anchor, uri).build()
}

pub(super) fn module_builder_for_path(anchor: &Moniker, path: &str) -> MonikerBuilder {
	let stem = strip_known_extension(path.trim_start_matches("./"));
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(crate::lang::kinds::LANG, b"ts");
	append_module_segments(&mut builder, stem);
	builder
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

pub(super) use crate::lang::callable::CallableSlot;

pub(super) fn anonymous_callback_name(node: Node<'_>) -> Vec<u8> {
	let p = node.start_position();
	format!("__cb_{}_{}", p.row, p.column).into_bytes()
}

pub(super) fn callable_param_slots(node: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	if let Some(params) = node.child_by_field_name("parameters") {
		let mut out = Vec::new();
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"required_parameter" | "optional_parameter" => {
					out.push(parameter_slot(child, source));
				}
				"rest_pattern" => out.push(CallableSlot {
					name: parameter_pattern_name(child, source),
					r#type: Vec::new(),
				}),
				_ => {}
			}
		}
		return out;
	}
	if let Some(p) = node.child_by_field_name("parameter") {
		return vec![CallableSlot {
			name: parameter_pattern_name(p, source),
			r#type: Vec::new(),
		}];
	}
	Vec::new()
}

fn parameter_slot(param: Node<'_>, source: &[u8]) -> CallableSlot {
	let r#type = param
		.child_by_field_name("type")
		.map(|annot| {
			let inner = annot.named_child(0).unwrap_or(annot);
			let text = inner.utf8_text(source).unwrap_or("");
			crate::lang::callable::normalize_type_text(text)
		})
		.unwrap_or_default();
	let name = param
		.child_by_field_name("pattern")
		.map(|p| parameter_pattern_name(p, source))
		.unwrap_or_default();
	CallableSlot { name, r#type }
}

fn parameter_pattern_name(node: Node<'_>, source: &[u8]) -> Vec<u8> {
	match node.kind() {
		"identifier" | "shorthand_property_identifier_pattern" => {
			node.utf8_text(source).unwrap_or("").as_bytes().to_vec()
		}
		_ => Vec::new(),
	}
}
