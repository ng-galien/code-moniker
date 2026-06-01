use tree_sitter::Node;

use crate::core::moniker::Moniker;
use crate::lang::tree_util::{find_named_child, node_slice};

use super::super::kinds;

pub(super) fn is_callable_kind(kind: &[u8]) -> bool {
	kind == kinds::FUNCTION || kind == kinds::METHOD || kind == kinds::CONSTRUCTOR
}

pub(super) fn is_intrinsic_jsx_tag(name: &[u8]) -> bool {
	matches!(name.first(), Some(b'a'..=b'z'))
}

pub(super) fn match_path_alias<'a>(pattern: &str, spec: &'a str) -> Option<&'a str> {
	if let Some(star) = pattern.find('*') {
		let prefix = &pattern[..star];
		let suffix = &pattern[star + 1..];
		if spec.len() >= prefix.len() + suffix.len()
			&& spec.starts_with(prefix)
			&& spec.ends_with(suffix)
		{
			return Some(&spec[prefix.len()..spec.len() - suffix.len()]);
		}
		None
	} else if pattern == spec {
		Some("")
	} else {
		None
	}
}

pub(super) fn apply_path_alias(template: &str, captured: &str) -> String {
	if let Some(star) = template.find('*') {
		let mut out = String::with_capacity(template.len() + captured.len());
		out.push_str(&template[..star]);
		out.push_str(captured);
		out.push_str(&template[star + 1..]);
		out
	} else {
		template.to_string()
	}
}

pub(super) fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::FUNCTION || last.kind == kinds::METHOD || last.kind == kinds::CONSTRUCTOR
}

pub(super) fn class_member_visibility(node: Node<'_>, source: &[u8]) -> &'static [u8] {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "accessibility_modifier" {
			return match c.utf8_text(source).unwrap_or("") {
				"private" => kinds::VIS_PRIVATE,
				"protected" => kinds::VIS_PROTECTED,
				_ => kinds::VIS_PUBLIC,
			};
		}
	}
	kinds::VIS_PUBLIC
}

pub(super) fn collect_binding_names<'src>(pat: Node<'src>, source: &'src [u8]) -> Vec<Vec<u8>> {
	fn rec<'src>(node: Node<'src>, source: &'src [u8], out: &mut Vec<Vec<u8>>) {
		match node.kind() {
			"identifier" | "shorthand_property_identifier_pattern" => {
				out.push(node_slice(node, source).to_vec());
			}
			"object_pattern" | "array_pattern" | "pair_pattern" | "rest_pattern"
			| "assignment_pattern" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					rec(c, source, out);
				}
			}
			_ => {}
		}
	}
	let mut out = Vec::new();
	rec(pat, source, &mut out);
	out
}

pub(super) fn receiver_hint<'a>(member_expr: Node<'_>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SUBSCRIPT, HINT_SUPER, HINT_THIS};
	let Some(obj) = member_expr.child_by_field_name("object") else {
		return b"";
	};
	match obj.kind() {
		"this" => HINT_THIS,
		"super" => HINT_SUPER,
		"identifier" => obj.utf8_text(source).unwrap_or("").as_bytes(),
		"call_expression" => HINT_CALL,
		"member_expression" => HINT_MEMBER,
		"subscript_expression" => HINT_SUBSCRIPT,
		_ => b"",
	}
}

pub(super) fn generic_short<'src>(node: Node<'src>, source: &'src [u8]) -> Option<&'src [u8]> {
	let inner = node.child_by_field_name("name").or_else(|| {
		let mut cursor = node.walk();
		node.named_children(&mut cursor).next()
	})?;
	match inner.kind() {
		"nested_type_identifier" => nested_type_short(inner, source),
		_ => Some(node_slice(inner, source)),
	}
}

pub(super) fn nested_type_short<'src>(node: Node<'src>, source: &'src [u8]) -> Option<&'src [u8]> {
	if let Some(name) = node.child_by_field_name("name") {
		return Some(node_slice(name, source));
	}
	let mut cursor = node.walk();
	let mut last: Option<&'src [u8]> = None;
	for c in node.named_children(&mut cursor) {
		if c.kind() == "type_identifier" || c.kind() == "identifier" {
			last = Some(node_slice(c, source));
		}
	}
	last
}

pub(super) fn nested_type_root<'src>(node: Node<'src>, source: &'src [u8]) -> Option<&'src [u8]> {
	let mut cursor = node.walk();
	for c in node.named_children(&mut cursor) {
		match c.kind() {
			"type_identifier" | "identifier" => return Some(node_slice(c, source)),
			"nested_type_identifier" => return nested_type_root(c, source),
			_ => {}
		}
	}
	None
}

pub(super) fn is_relative_specifier(spec: &str) -> bool {
	spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../")
}

pub(super) fn import_confidence(spec: &str) -> &'static [u8] {
	if is_relative_specifier(spec) {
		kinds::CONF_IMPORTED
	} else {
		kinds::CONF_EXTERNAL
	}
}

pub(super) fn first_identifier_text<'src>(node: Node<'src>, source: &'src [u8]) -> &'src [u8] {
	find_named_child(node, "identifier")
		.map(|c| node_slice(c, source))
		.unwrap_or(b"")
}

pub(super) fn unquote_string_literal<'src>(node: Node<'_>, source: &'src [u8]) -> &'src str {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "string_fragment"
			&& let Ok(s) = c.utf8_text(source)
		{
			return s;
		}
	}
	node.utf8_text(source)
		.unwrap_or("")
		.trim_matches(|c| c == '"' || c == '\'' || c == '`')
}
