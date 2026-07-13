use tree_sitter::Node;

use crate::core::moniker::Moniker;
use crate::lang::callable::extend_segment;
use crate::lang::sdk::TypeExpr;
use crate::lang::tree_util::node_slice;

use super::super::kinds;
use super::builtins::is_go_primitive;
use super::discover::GoDiscover;
use super::imports::ImportedPackage;
use super::syntax::named_children;

pub(super) fn lookup_import<'a>(
	state: &'a GoDiscover<'_>,
	name: &[u8],
) -> Option<&'a ImportedPackage> {
	state.imports.iter().find(|import| import.alias == name)
}

// File-local type names win; anything else is a same-package name claim the
// linkage layer arbitrates (same target shape as the legacy extractor, so
// existing monikers stay stable).
pub(super) fn resolve_type_target(
	state: &GoDiscover<'_>,
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

pub(super) fn resolve_type_node(
	state: &GoDiscover<'_>,
	node: Node<'_>,
) -> Option<(Moniker, &'static [u8])> {
	match node.kind() {
		"type_identifier" => {
			let name = node_slice(node, state.source);
			if name.is_empty() || is_go_primitive(name) {
				return None;
			}
			Some(resolve_type_target(state, name, kinds::STRUCT))
		}
		"qualified_type" => {
			let pkg = node
				.child_by_field_name("package")
				.map(|n| node_slice(n, state.source))
				.unwrap_or(b"");
			let name_node = node.child_by_field_name("name")?;
			let name = node_slice(name_node, state.source);
			if name.is_empty() {
				return None;
			}
			if let Some(entry) = lookup_import(state, pkg) {
				Some((
					extend_segment(&entry.target, kinds::STRUCT, name),
					entry.confidence,
				))
			} else {
				Some(resolve_type_target(state, name, kinds::STRUCT))
			}
		}
		_ => None,
	}
}

pub(super) fn type_expr(state: &GoDiscover<'_>, node: Node<'_>) -> Option<TypeExpr> {
	match node.kind() {
		"type_identifier" | "qualified_type" => {
			resolve_type_node(state, node).map(|(target, _)| TypeExpr::resolved(target))
		}
		"pointer_type" => {
			let inner = named_children(node).next()?;
			type_expr(state, inner).map(|inner| TypeExpr::Pointer(Box::new(inner)))
		}
		"slice_type" | "array_type" => {
			let element = node
				.child_by_field_name("element")
				.or_else(|| named_children(node).last())?;
			type_expr(state, element).map(|element| TypeExpr::Array(Box::new(element)))
		}
		"generic_type" => {
			let base = node.child_by_field_name("type")?;
			type_expr(state, base).map(|base| TypeExpr::Generic {
				base: Box::new(base),
				args: Vec::new(),
			})
		}
		"parenthesized_type" => {
			let inner = named_children(node).next()?;
			type_expr(state, inner)
		}
		_ => None,
	}
}

// A Go result is either a single type or a parameter list; the value type of
// the idiomatic `(T, error)` pair is the first non-error slot.
pub(super) fn result_type_expr(state: &GoDiscover<'_>, result: Node<'_>) -> Option<TypeExpr> {
	if result.kind() != "parameter_list" {
		return type_expr(state, result);
	}
	for param in named_children(result) {
		let type_node = match param.kind() {
			"parameter_declaration" => param.child_by_field_name("type"),
			_ => None,
		};
		let Some(type_node) = type_node else { continue };
		if node_slice(type_node, state.source) == b"error" {
			continue;
		}
		if let Some(ty) = type_expr(state, type_node) {
			return Some(ty);
		}
	}
	None
}

pub(super) fn external_target_shape(target: &Moniker) -> bool {
	target
		.as_view()
		.segments()
		.any(|segment| segment.kind == kinds::EXTERNAL_PKG)
}

pub(super) fn owner_confidence(state: &GoDiscover<'_>, owner: &Moniker) -> &'static [u8] {
	if external_target_shape(owner) {
		kinds::CONF_EXTERNAL
	} else if state.type_table.values().any(|target| target == owner) {
		kinds::CONF_RESOLVED
	} else {
		kinds::CONF_NAME_MATCH
	}
}
