use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::tree_util::find_named_child;

use super::kinds;

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let stem = strip_cs_extension(uri);
	let mut b = MonikerBuilder::from_view(anchor.as_view());
	b.segment(crate::lang::kinds::LANG, b"cs");
	crate::lang::callable::append_dir_module_segments(&mut b, stem, kinds::PACKAGE, kinds::MODULE);
	b.build()
}

pub(super) fn strip_cs_extension(uri: &str) -> &str {
	uri.strip_suffix(".cs").unwrap_or(uri)
}

pub(super) use crate::lang::callable::CallableSlot;
use crate::lang::tree_util::node_slice;

pub(super) fn parameter_slots(callable: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let params = callable
		.child_by_field_name("parameters")
		.or_else(|| find_named_child(callable, "parameter_list"));
	let Some(params) = params else {
		return Vec::new();
	};
	parameter_list_slots(params, source)
}

pub(super) fn parameter_list_slots(params: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let mut out = Vec::new();
	let mut has_params_modifier = false;
	let mut cursor = params.walk();
	for c in params.children(&mut cursor) {
		match c.kind() {
			"parameter" => {
				let r#type = c
					.child_by_field_name("type")
					.and_then(|t| t.utf8_text(source).ok())
					.map(crate::lang::callable::normalize_type_text)
					.unwrap_or_default();
				let name = c
					.child_by_field_name("name")
					.map(|n| node_slice(n, source).to_vec())
					.unwrap_or_default();
				out.push(CallableSlot { name, r#type });
			}
			"params" => has_params_modifier = true,
			_ => {}
		}
	}
	if has_params_modifier {
		out.push(CallableSlot {
			name: Vec::new(),
			r#type: b"...".to_vec(),
		});
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn strip_cs_extension_drops_dot_cs() {
		assert_eq!(strip_cs_extension("Foo.cs"), "Foo");
		assert_eq!(strip_cs_extension("a/b/Foo.cs"), "a/b/Foo");
		assert_eq!(strip_cs_extension("Foo"), "Foo");
	}
}
