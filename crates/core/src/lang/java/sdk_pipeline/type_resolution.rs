use tree_sitter::Node;

use crate::core::moniker::Moniker;
use crate::lang::callable::extend_segment;
use crate::lang::sdk::{TypeEnv, TypeExpr};

use super::super::kinds;
use super::builtins;
use super::discover::JavaDiscover;
use super::imports::{java_lang_target, same_package_symbol_target};
use super::syntax::type_name;

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

pub(super) fn type_expr(
	state: &JavaDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<TypeExpr> {
	let name = type_name(node, state.source)?;
	if builtins::is_primitive_type(&name) || builtins::is_inferred_local_type(&name) {
		return None;
	}
	if is_type_param_in_scope(state, scope, &name) {
		return Some(TypeExpr::TypeParam(name));
	}
	Some(TypeExpr::resolved(
		resolve_type_target(state, &name, kinds::CLASS).0,
	))
}

fn type_param_scope_visible(owner: &Moniker, scope: &Moniker) -> bool {
	owner == scope || owner.as_view().is_ancestor_of(&scope.as_view())
}
