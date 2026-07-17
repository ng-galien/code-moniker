use tree_sitter::Node;

use crate::core::moniker::Moniker;
use crate::lang::callable::extend_segment;
use crate::lang::sdk::TypeExpr;
use crate::lang::tree_util::node_slice;

use super::super::kinds;
use super::discover::CDiscover;
use super::syntax::named_children;

pub(super) fn is_c_primitive(name: &[u8]) -> bool {
	matches!(
		name,
		b"void"
			| b"char" | b"short"
			| b"int" | b"long"
			| b"float"
			| b"double"
			| b"signed"
			| b"unsigned"
			| b"_Bool"
			| b"bool" | b"size_t"
			| b"ssize_t"
			| b"ptrdiff_t"
			| b"intptr_t"
			| b"uintptr_t"
			| b"int8_t"
			| b"int16_t"
			| b"int32_t"
			| b"int64_t"
			| b"uint8_t"
			| b"uint16_t"
			| b"uint32_t"
			| b"uint64_t"
			| b"off_t"
			| b"time_t"
			| b"pid_t"
			| b"uid_t"
			| b"FILE" | b"va_list"
	)
}

// File-local type names win; anything else stays a same-project name claim
// that the C linkage strategy arbitrates across files.
pub(super) fn resolve_type_target(
	state: &CDiscover<'_>,
	name: &[u8],
	fallback_kind: &'static [u8],
) -> (Moniker, &'static [u8]) {
	if let Some(target) = state.type_table.get(name) {
		return (target.clone(), kinds::CONF_RESOLVED);
	}
	(
		extend_segment(&state.root, fallback_kind, name),
		kinds::CONF_NAME_MATCH,
	)
}

// TypeExpr for a declaration's type node plus its declarator pointer depth.
pub(super) fn type_expr_for(
	state: &CDiscover<'_>,
	type_node: Node<'_>,
	pointer_depth: usize,
) -> Option<TypeExpr> {
	let base = base_type_expr(state, type_node)?;
	let mut expr = base;
	for _ in 0..pointer_depth {
		expr = TypeExpr::Pointer(Box::new(expr));
	}
	Some(expr)
}

fn base_type_expr(state: &CDiscover<'_>, node: Node<'_>) -> Option<TypeExpr> {
	match node.kind() {
		"type_identifier" => {
			let name = node_slice(node, state.source);
			if name.is_empty() || is_c_primitive(name) {
				return None;
			}
			Some(TypeExpr::resolved(
				resolve_type_target(state, name, kinds::TYPE).0,
			))
		}
		"struct_specifier" | "union_specifier" => {
			let name_node = node.child_by_field_name("name")?;
			let name = node_slice(name_node, state.source);
			if name.is_empty() {
				return None;
			}
			Some(TypeExpr::resolved(
				resolve_type_target(state, name, kinds::STRUCT).0,
			))
		}
		"enum_specifier" => {
			let name_node = node.child_by_field_name("name")?;
			Some(TypeExpr::resolved(
				resolve_type_target(state, node_slice(name_node, state.source), kinds::ENUM).0,
			))
		}
		"sized_type_specifier" | "primitive_type" => None,
		_ => named_children(node)
			.next()
			.and_then(|inner| base_type_expr(state, inner)),
	}
}
