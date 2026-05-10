use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

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

pub(super) use crate::lang::callable::{
	extend_callable_arity, extend_callable_typed, extend_segment, extend_segment_u32,
};

pub(super) fn node_position(node: Node<'_>) -> (u32, u32) {
	(node.start_byte() as u32, node.end_byte() as u32)
}

pub(super) fn parameter_types(callable: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let params = callable
		.child_by_field_name("parameters")
		.or_else(|| find_named_child(callable, "parameter_list"));
	let Some(params) = params else {
		return Vec::new();
	};
	parameter_list_types(params, source)
}

pub(super) fn parameter_list_types(params: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let mut out = Vec::new();
	let mut has_params_modifier = false;
	let mut cursor = params.walk();
	for c in params.children(&mut cursor) {
		match c.kind() {
			"parameter" => {
				let ty = c
					.child_by_field_name("type")
					.and_then(|t| t.utf8_text(source).ok())
					.map(crate::lang::callable::normalize_type_text)
					.unwrap_or_else(|| b"_".to_vec());
				out.push(ty);
			}
			"params" => has_params_modifier = true,
			_ => {}
		}
	}
	if has_params_modifier {
		out.push(b"...".to_vec());
	}
	out
}

pub(super) fn find_named_child<'tree>(parent: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
	let mut cursor = parent.walk();
	parent
		.named_children(&mut cursor)
		.find(|c| c.kind() == kind)
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
