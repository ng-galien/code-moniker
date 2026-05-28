use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::callable::extend_segment;
use crate::lang::sdk::{TypeEnv, TypeExpr};

use super::super::kinds;
use super::builtins;
use super::discover::JavaDiscover;
use super::imports::{
	external_or_imported, external_package_target, java_lang_target, same_package_symbol_target,
};
use super::syntax::{named_children, type_path};

pub(super) fn type_env_for_scope(state: &JavaDiscover<'_>, scope: &Moniker) -> TypeEnv {
	let mut env = TypeEnv::default();
	for (owner, params) in &state.type_params {
		if type_param_scope_visible(owner, scope) {
			for param in params {
				env.bind_type_param(param.clone());
			}
		}
	}
	env
}

pub(super) fn is_type_param_in_scope(
	state: &JavaDiscover<'_>,
	scope: &Moniker,
	name: &[u8],
) -> bool {
	state.type_params.iter().any(|(owner, params)| {
		type_param_scope_visible(owner, scope) && params.iter().any(|param| param == name)
	})
}

pub(super) fn lookup_known_type_name(
	state: &JavaDiscover<'_>,
	name: &[u8],
) -> Option<(Moniker, &'static [u8])> {
	if let Some(target) = state.type_table.get(name) {
		return Some((target.clone(), kinds::CONF_RESOLVED));
	}
	if let Some(import) = state.imports.iter().find(|import| import.name == name) {
		if import.is_static {
			return None;
		}
		return Some((import.target.clone(), import.confidence));
	}
	if builtins::is_java_lang_type(name) {
		return Some((java_lang_target(&state.root, name), kinds::CONF_EXTERNAL));
	}
	None
}

pub(super) fn same_package_type_target(state: &JavaDiscover<'_>, name: &[u8]) -> Option<Moniker> {
	name.first()
		.is_some_and(u8::is_ascii_uppercase)
		.then(|| same_package_symbol_target(&state.root, name))
}

pub(super) fn resolve_type_target(
	state: &JavaDiscover<'_>,
	name: &[u8],
	fallback_kind: &'static [u8],
) -> (Moniker, &'static [u8]) {
	if let Some(found) = lookup_known_type_name(state, name) {
		return found;
	}
	if builtins::is_primitive_type(name) || builtins::is_inferred_local_type(name) {
		return (state.root.clone(), kinds::CONF_RESOLVED);
	}
	let target = if fallback_kind == kinds::ANNOTATION_TYPE {
		extend_segment(&state.root, fallback_kind, name)
	} else {
		same_package_symbol_target(&state.root, name)
	};
	(target, kinds::CONF_NAME_MATCH)
}

pub(super) fn resolve_type_path(
	state: &JavaDiscover<'_>,
	pieces: &[Vec<u8>],
	fallback_kind: &'static [u8],
) -> (Moniker, &'static [u8]) {
	let Some((head, tail)) = pieces.split_first() else {
		return (state.root.clone(), kinds::CONF_NAME_MATCH);
	};
	if tail.is_empty() {
		return resolve_type_target(state, head, fallback_kind);
	}
	if let Some((base, confidence)) = lookup_known_type_name(state, head) {
		return (append_path_segments(&base, tail), confidence);
	}
	if head.first().is_some_and(u8::is_ascii_uppercase) {
		let base = same_package_symbol_target(&state.root, head);
		return (append_path_segments(&base, tail), kinds::CONF_NAME_MATCH);
	}
	let as_str = pieces
		.iter()
		.map(|piece| std::str::from_utf8(piece).unwrap_or(""))
		.collect::<Vec<_>>();
	let confidence = external_or_imported(&as_str);
	(
		external_package_target(state.root.as_view().project(), &as_str),
		confidence,
	)
}

pub(super) fn type_expr(
	state: &JavaDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<TypeExpr> {
	match node.kind() {
		"array_type" => {
			return node
				.child_by_field_name("element")
				.and_then(|element| type_expr(state, element, scope))
				.map(|element| TypeExpr::Array(Box::new(element)));
		}
		"generic_type" => return generic_type_expr(state, node, scope),
		_ => {}
	}
	let path = type_path(node, state.source)?;
	let name = path.last()?;
	if builtins::is_primitive_type(name) || builtins::is_inferred_local_type(name) {
		return None;
	}
	if path.len() == 1 && is_type_param_in_scope(state, scope, name) {
		return Some(TypeExpr::TypeParam(name.clone()));
	}
	Some(TypeExpr::resolved(
		resolve_type_path(state, &path, kinds::CLASS).0,
	))
}

fn generic_type_expr(
	state: &JavaDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<TypeExpr> {
	let base_node = node.child_by_field_name("type").or_else(|| {
		named_children(node)
			.find(|child| matches!(child.kind(), "type_identifier" | "scoped_type_identifier"))
	})?;
	let base = type_expr(state, base_node, scope)?;
	let args = node
		.child_by_field_name("type_arguments")
		.or_else(|| named_children(node).find(|child| child.kind() == "type_arguments"))
		.into_iter()
		.flat_map(named_children)
		.filter_map(|arg| type_expr(state, arg, scope))
		.collect::<Vec<_>>();
	Some(TypeExpr::Generic {
		base: Box::new(base),
		args,
	})
}

fn type_param_scope_visible(owner: &Moniker, scope: &Moniker) -> bool {
	owner == scope || owner.as_view().is_ancestor_of(&scope.as_view())
}

fn append_path_segments(base: &Moniker, tail: &[Vec<u8>]) -> Moniker {
	let mut builder = MonikerBuilder::from_view(base.as_view());
	for piece in tail {
		builder.segment(kinds::PATH, piece);
	}
	builder.build()
}
