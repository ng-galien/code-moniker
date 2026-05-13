use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let stem = strip_rs_extension(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(crate::lang::kinds::LANG, b"rs");
	crate::lang::callable::append_dir_module_segments(
		&mut builder,
		stem,
		kinds::DIR,
		kinds::MODULE,
	);
	builder.build()
}

pub(super) fn strip_rs_extension(uri: &str) -> &str {
	uri.strip_suffix(".rs").unwrap_or(uri)
}

pub(super) use crate::lang::callable::CallableSlot;

pub(super) fn function_param_slots(node: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let Some(params) = node.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		match child.kind() {
			"parameter" => {
				let r#type = child
					.child_by_field_name("type")
					.and_then(|n| n.utf8_text(source).ok())
					.map(crate::lang::callable::normalize_type_text)
					.unwrap_or_default();
				let name = child
					.child_by_field_name("pattern")
					.filter(|p| p.kind() == "identifier")
					.and_then(|p| p.utf8_text(source).ok())
					.map(|s| s.as_bytes().to_vec())
					.unwrap_or_default();
				out.push(CallableSlot { name, r#type });
			}
			"variadic_parameter" => out.push(CallableSlot {
				name: Vec::new(),
				r#type: b"...".to_vec(),
			}),
			"self_parameter" => {}
			_ => {}
		}
	}
	out
}

pub(super) fn closure_param_slots(closure: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let Some(params) = closure.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		if child.kind() == "parameter" {
			let r#type = child
				.child_by_field_name("type")
				.and_then(|n| n.utf8_text(source).ok())
				.map(crate::lang::callable::normalize_type_text)
				.unwrap_or_default();
			let name = child
				.child_by_field_name("pattern")
				.filter(|p| p.kind() == "identifier")
				.and_then(|p| p.utf8_text(source).ok())
				.map(|s| s.as_bytes().to_vec())
				.unwrap_or_default();
			out.push(CallableSlot { name, r#type });
		} else if child.kind() == "identifier" {
			let name = child
				.utf8_text(source)
				.map(|s| s.as_bytes().to_vec())
				.unwrap_or_default();
			out.push(CallableSlot {
				name,
				r#type: Vec::new(),
			});
		} else {
			out.push(CallableSlot::default());
		}
	}
	out
}

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
