use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let stem = strip_go_extension(uri);
	let mut b = MonikerBuilder::from_view(anchor.as_view());
	b.segment(crate::lang::kinds::LANG, b"go");
	crate::lang::callable::append_dir_module_segments(&mut b, stem, kinds::PACKAGE, kinds::MODULE);
	b.build()
}

pub(super) fn strip_go_extension(uri: &str) -> &str {
	uri.strip_suffix(".go").unwrap_or(uri)
}

pub(super) use crate::lang::callable::CallableSlot;

pub(super) fn function_param_slots(node: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let Some(params) = node.child_by_field_name("parameters") else {
		return Vec::new();
	};
	flatten_param_slots(params, source)
}

pub(super) fn flatten_param_slots(params: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let mut out = Vec::new();
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		match child.kind() {
			"parameter_declaration" => {
				let ty = child
					.child_by_field_name("type")
					.and_then(|n| n.utf8_text(source).ok())
					.map(crate::lang::callable::normalize_type_text)
					.unwrap_or_default();
				let names = parameter_name_bytes(child, source);
				if names.is_empty() {
					out.push(CallableSlot {
						name: Vec::new(),
						r#type: ty,
					});
				} else {
					for name in names {
						out.push(CallableSlot {
							name,
							r#type: ty.clone(),
						});
					}
				}
			}
			"variadic_parameter_declaration" => {
				out.push(CallableSlot {
					name: Vec::new(),
					r#type: b"...".to_vec(),
				});
			}
			_ => {}
		}
	}
	out
}

fn parameter_name_bytes(decl: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let mut out = Vec::new();
	let mut cursor = decl.walk();
	for child in decl.named_children(&mut cursor) {
		if child.kind() == "identifier" {
			let name = child.utf8_text(source).unwrap_or("").as_bytes();
			if !name.is_empty() && name != b"_" {
				out.push(name.to_vec());
			}
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn strip_go_extension_drops_dot_go() {
		assert_eq!(strip_go_extension("foo.go"), "foo");
		assert_eq!(strip_go_extension("a/b/foo.go"), "a/b/foo");
		assert_eq!(strip_go_extension("foo"), "foo");
	}
}
