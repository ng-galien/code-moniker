use tree_sitter::Node;

use crate::core::moniker::Moniker;
use crate::lang::callable::extend_segment;
use crate::lang::sdk::{RefHints, ResolvedRef, TypeEnv, TypeExpr};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::builtins::{is_libc_function, libc_function_target};
use super::defs::is_preproc_container;
use super::discover::CDiscover;
use super::imports::collect_include;
use super::syntax::{
	argument_count, declarator_info, named_children, parameter_slots, receiver_hint_bytes,
};
use super::type_resolution::{is_c_primitive, resolve_type_target, type_expr_for};

pub(super) fn collect_refs(state: &mut CDiscover<'_>, root: Node<'_>, scope: &Moniker) {
	for child in named_children(root) {
		match child.kind() {
			"preproc_include" => collect_include(state, child, scope),
			"function_definition" => function_refs(state, child, scope),
			"declaration" => module_declaration_refs(state, child, scope),
			"type_definition" => typedef_refs(state, child, scope),
			"struct_specifier" | "union_specifier" | "enum_specifier" => {
				specifier_type_refs_for(state, child, scope);
			}
			kind if is_preproc_container(kind) => collect_refs(state, child, scope),
			"ERROR" => collect_refs(state, child, scope),
			_ => {}
		}
	}
}

fn function_refs(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(declarator) = node.child_by_field_name("declarator") else {
		return;
	};
	let Some(info) = declarator_info(declarator, state.source) else {
		return;
	};
	let slots = info
		.parameters
		.map(|params| parameter_slots(params, state.source))
		.unwrap_or_default();
	let callable =
		crate::lang::callable::extend_callable_slots(scope, kinds::FUNC, info.name, &slots);

	if let Some(params) = info.parameters {
		for param in named_children(params) {
			if let Some(ty) = param.child_by_field_name("type") {
				emit_uses_type(state, ty, &callable);
			}
		}
	}
	if let Some(return_type) = node.child_by_field_name("type") {
		emit_uses_type(state, return_type, &callable);
		push_returns_type(state, &callable, return_type, info.pointer_depth);
	}

	let Some(body) = node.child_by_field_name("body") else {
		return;
	};
	let mut env = TypeEnv::default();
	if let Some(params) = info.parameters {
		bind_params(state, params, &mut env);
	}
	expr_refs(state, body, &callable, &mut env);
}

fn push_returns_type(
	state: &mut CDiscover<'_>,
	callable: &Moniker,
	return_type: Node<'_>,
	pointer_depth: usize,
) {
	let Some(target) = type_expr_for(state, return_type, pointer_depth)
		.and_then(|ty| ty.receiver_owner().cloned())
	else {
		return;
	};
	state.push_ref(ResolvedRef {
		source: callable.clone(),
		target,
		kind: kinds::RETURNS_TYPE,
		position: Some(node_position(return_type)),
		confidence: kinds::CONF_RESOLVED,
		hints: RefHints::default(),
	});
}

fn bind_params(state: &CDiscover<'_>, params: Node<'_>, env: &mut TypeEnv) {
	for param in named_children(params) {
		if param.kind() != "parameter_declaration" {
			continue;
		}
		let Some(info) = param
			.child_by_field_name("declarator")
			.and_then(|decl| declarator_info(decl, state.source))
		else {
			continue;
		};
		let ty = param
			.child_by_field_name("type")
			.and_then(|type_node| type_expr_for(state, type_node, info.pointer_depth))
			.unwrap_or(TypeExpr::Unknown);
		env.bind_local(info.name, ty);
	}
}

fn bind_declaration_locals(state: &CDiscover<'_>, node: Node<'_>, env: &mut TypeEnv) {
	let type_node = node.child_by_field_name("type");
	let mut cursor = node.walk();
	for declarator in node.children_by_field_name("declarator", &mut cursor) {
		let (target, value) = match declarator.kind() {
			"init_declarator" => (
				declarator.child_by_field_name("declarator"),
				declarator.child_by_field_name("value"),
			),
			_ => (Some(declarator), None),
		};
		let Some(info) = target.and_then(|decl| declarator_info(decl, state.source)) else {
			continue;
		};
		let declared = type_node.and_then(|ty| type_expr_for(state, ty, info.pointer_depth));
		let inferred = value.and_then(|value| infer_value_type(state, value, env));
		env.bind_local(
			info.name,
			declared.or(inferred).unwrap_or(TypeExpr::Unknown),
		);
	}
}

fn infer_value_type(state: &CDiscover<'_>, value: Node<'_>, env: &TypeEnv) -> Option<TypeExpr> {
	match value.kind() {
		"call_expression" => infer_call_type(state, value, env),
		"cast_expression" => value
			.child_by_field_name("type")
			.and_then(|descriptor| type_descriptor_expr(state, descriptor)),
		"identifier" => env.resolve_local(node_slice(value, state.source)).cloned(),
		"pointer_expression" | "parenthesized_expression" => named_children(value)
			.next()
			.and_then(|inner| infer_value_type(state, inner, env)),
		_ => None,
	}
}

fn infer_call_type(state: &CDiscover<'_>, call: Node<'_>, env: &TypeEnv) -> Option<TypeExpr> {
	let callee = call.child_by_field_name("function")?;
	match callee.kind() {
		"identifier" => state
			.return_types
			.get(node_slice(callee, state.source))
			.cloned(),
		"field_expression" => {
			let receiver = receiver_type_expr(state, callee, env)?;
			match receiver {
				TypeExpr::Unknown => None,
				other => Some(other),
			}
		}
		_ => {
			let _ = env;
			None
		}
	}
}

fn type_descriptor_expr(state: &CDiscover<'_>, descriptor: Node<'_>) -> Option<TypeExpr> {
	let type_node = descriptor.child_by_field_name("type")?;
	let pointer_depth = descriptor
		.child_by_field_name("declarator")
		.map(abstract_pointer_depth)
		.unwrap_or(0);
	type_expr_for(state, type_node, pointer_depth)
}

fn abstract_pointer_depth(declarator: Node<'_>) -> usize {
	let mut depth = 0;
	let mut node = Some(declarator);
	while let Some(current) = node {
		if current.kind() == "abstract_pointer_declarator" {
			depth += 1;
			node = named_children(current).next();
		} else {
			break;
		}
	}
	depth
}

// The type an expression carries when used as a `->`/`.` receiver.
fn receiver_type_expr(state: &CDiscover<'_>, node: Node<'_>, env: &TypeEnv) -> Option<TypeExpr> {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, state.source);
			env.resolve_local(name)
				.cloned()
				.or_else(|| state.var_types.get(name).cloned())
		}
		"field_expression" => {
			let argument = node.child_by_field_name("argument")?;
			let field = node.child_by_field_name("field")?;
			let receiver = receiver_type_expr(state, argument, env)?;
			let owner = receiver.receiver_owner()?.clone();
			state
				.field_types
				.get(&(owner, node_slice(field, state.source).to_vec()))
				.cloned()
		}
		"call_expression" => infer_call_type(state, node, env),
		"cast_expression" => node
			.child_by_field_name("type")
			.and_then(|descriptor| type_descriptor_expr(state, descriptor)),
		"pointer_expression" | "parenthesized_expression" => named_children(node)
			.next()
			.and_then(|inner| receiver_type_expr(state, inner, env)),
		"subscript_expression" => node
			.child_by_field_name("argument")
			.and_then(|argument| receiver_type_expr(state, argument, env)),
		_ => None,
	}
}

fn expr_refs(state: &mut CDiscover<'_>, node: Node<'_>, source: &Moniker, env: &mut TypeEnv) {
	match node.kind() {
		"compound_statement" => {
			let mut block_env = env.clone();
			for child in named_children(node) {
				expr_refs(state, child, source, &mut block_env);
			}
			return;
		}
		"for_statement" | "if_statement" | "switch_statement" | "while_statement"
		| "do_statement" => {
			let mut control_env = env.clone();
			for child in named_children(node) {
				expr_refs(state, child, source, &mut control_env);
			}
			return;
		}
		"call_expression" => {
			call_ref(state, node, source, env);
			return;
		}
		"field_expression" => {
			field_read_ref(state, node, source, env);
			if let Some(argument) = node.child_by_field_name("argument") {
				expr_refs(state, argument, source, env);
			}
			return;
		}
		"assignment_expression" => {
			if let Some(right) = node.child_by_field_name("right") {
				expr_refs(state, right, source, env);
			}
			return;
		}
		"identifier" => {
			identifier_read_ref(state, node, source, env);
			return;
		}
		"cast_expression" => {
			if let Some(descriptor) = node.child_by_field_name("type") {
				emit_type_descriptor_uses(state, descriptor, source);
			}
			if let Some(value) = node.child_by_field_name("value") {
				expr_refs(state, value, source, env);
			}
			return;
		}
		"sizeof_expression" | "alignof_expression" => {
			if let Some(descriptor) =
				named_children(node).find(|child| child.kind() == "type_descriptor")
			{
				emit_type_descriptor_uses(state, descriptor, source);
			}
			return;
		}
		"declaration" => {
			if let Some(ty) = node.child_by_field_name("type") {
				emit_uses_type(state, ty, source);
			}
			let mut cursor = node.walk();
			for declarator in node.children_by_field_name("declarator", &mut cursor) {
				if declarator.kind() == "init_declarator"
					&& let Some(value) = declarator.child_by_field_name("value")
				{
					expr_refs(state, value, source, env);
				}
			}
			bind_declaration_locals(state, node, env);
			return;
		}
		_ => {}
	}
	for child in named_children(node) {
		expr_refs(state, child, source, env);
	}
}

fn emit_type_descriptor_uses(state: &mut CDiscover<'_>, descriptor: Node<'_>, source: &Moniker) {
	if let Some(type_node) = descriptor.child_by_field_name("type") {
		emit_uses_type(state, type_node, source);
	}
}

#[derive(Clone, Copy)]
struct CallSite<'a> {
	source: &'a Moniker,
	position: (u32, u32),
	name: &'a [u8],
	arity: usize,
}

fn call_ref(state: &mut CDiscover<'_>, call: Node<'_>, source: &Moniker, env: &mut TypeEnv) {
	let site = CallSite {
		source,
		position: node_position(call),
		name: b"",
		arity: call
			.child_by_field_name("arguments")
			.map(argument_count)
			.unwrap_or(0),
	};
	if let Some(callee) = call.child_by_field_name("function") {
		match callee.kind() {
			"identifier" => {
				let site = CallSite {
					name: node_slice(callee, state.source),
					..site
				};
				simple_call_ref(state, &site, env);
			}
			"field_expression" => field_call_ref(state, callee, site, env),
			_ => expr_refs(state, callee, source, env),
		}
	}
	if let Some(arguments) = call.child_by_field_name("arguments") {
		expr_refs(state, arguments, source, env);
	}
}

fn simple_call_ref(state: &mut CDiscover<'_>, site: &CallSite<'_>, env: &TypeEnv) {
	let name = site.name;
	if name.is_empty() {
		return;
	}
	if env.resolve_local(name).is_some() {
		let target = extend_segment(site.source, kinds::LOCAL, name);
		let confidence = if state.deep {
			kinds::CONF_LOCAL
		} else {
			kinds::CONF_NAME_MATCH
		};
		push_call(state, site, target, confidence, b"");
		return;
	}
	if let Some(segment) = state.macros.get(name) {
		let target = extend_segment(&state.root, kinds::MACRO, segment);
		push_call(state, site, target, kinds::CONF_RESOLVED, b"");
		return;
	}
	if let Some(segment) = state.callables.get(name) {
		let target = extend_segment(&state.root, kinds::FUNC, segment);
		push_call(state, site, target, kinds::CONF_RESOLVED, b"");
		return;
	}
	if is_libc_function(name) {
		let target = libc_function_target(&state.root, name);
		push_call(state, site, target, kinds::CONF_EXTERNAL, b"");
		return;
	}
	let target = extend_segment(&state.root, kinds::FUNC, name);
	push_call(state, site, target, kinds::CONF_NAME_MATCH, b"");
}

fn identifier_read_ref(state: &mut CDiscover<'_>, node: Node<'_>, source: &Moniker, env: &TypeEnv) {
	let name = node_slice(node, state.source);
	if name.is_empty() {
		return;
	}
	let local = env.resolve_local(name).is_some();
	if local && !state.deep {
		return;
	}
	let (target, confidence) = if local {
		(
			extend_segment(source, kinds::LOCAL, name),
			kinds::CONF_LOCAL,
		)
	} else if let Some(definition) = state.defs.iter().find(|definition| {
		definition.name == name
			&& matches!(
				definition.kind,
				kinds::CONST | kinds::VAR | kinds::ENUM_CONSTANT
			)
	}) {
		(definition.moniker.clone(), kinds::CONF_RESOLVED)
	} else {
		(
			extend_segment(&state.root, kinds::VAR, name),
			kinds::CONF_NAME_MATCH,
		)
	};
	state.push_ref(ResolvedRef {
		source: source.clone(),
		target,
		kind: kinds::READS,
		position: Some(node_position(node)),
		confidence,
		hints: RefHints::default(),
	});
}

fn field_read_ref(state: &mut CDiscover<'_>, node: Node<'_>, source: &Moniker, env: &TypeEnv) {
	let Some(field) = node.child_by_field_name("field") else {
		return;
	};
	let name = node_slice(field, state.source);
	let owner = node
		.child_by_field_name("argument")
		.and_then(|argument| receiver_type_expr(state, argument, env))
		.and_then(|ty| ty.receiver_owner().cloned());
	let resolved = owner.as_ref().is_some_and(|owner| {
		state
			.field_types
			.contains_key(&(owner.clone(), name.to_vec()))
	});
	let target = owner
		.map(|owner| extend_segment(&owner, kinds::FIELD, name))
		.unwrap_or_else(|| extend_segment(&state.root, kinds::FIELD, name));
	state.push_ref(ResolvedRef {
		source: source.clone(),
		target,
		kind: kinds::READS,
		position: Some(node_position(field)),
		confidence: if resolved {
			kinds::CONF_RESOLVED
		} else {
			kinds::CONF_NAME_MATCH
		},
		hints: RefHints::default(),
	});
}

// A call through a struct member is a function-pointer dispatch: when the
// receiver types to a known field the call binds to that field; otherwise the
// fact stays a method_call with hints for the semantic layer to arbitrate.
fn field_call_ref(
	state: &mut CDiscover<'_>,
	callee: Node<'_>,
	site: CallSite<'_>,
	env: &mut TypeEnv,
) {
	let Some(field) = callee.child_by_field_name("field") else {
		return;
	};
	let name = node_slice(field, state.source);
	if name.is_empty() {
		return;
	}
	let site = CallSite { name, ..site };
	let argument = callee.child_by_field_name("argument");
	let hint = argument
		.map(|arg| receiver_hint_bytes(arg, state.source))
		.unwrap_or(b"");
	let owner = argument
		.and_then(|arg| receiver_type_expr(state, arg, env))
		.and_then(|ty| ty.receiver_owner().cloned());
	if let Some(owner) = &owner
		&& state
			.field_types
			.contains_key(&(owner.clone(), name.to_vec()))
	{
		let target = extend_segment(owner, kinds::FIELD, name);
		push_call(state, &site, target, kinds::CONF_RESOLVED, hint);
	} else {
		let target = owner
			.map(|owner| extend_segment(&owner, kinds::FIELD, name))
			.unwrap_or_else(|| extend_segment(&state.root, kinds::FIELD, name));
		state.push_ref(ResolvedRef {
			source: site.source.clone(),
			target,
			kind: kinds::METHOD_CALL,
			position: Some(site.position),
			confidence: kinds::CONF_NAME_MATCH,
			hints: call_hints(site.name, site.arity, hint),
		});
	}
	if let Some(arg) = argument {
		expr_refs(state, arg, site.source, env);
	}
}

fn push_call(
	state: &mut CDiscover<'_>,
	site: &CallSite<'_>,
	target: Moniker,
	confidence: &'static [u8],
	hint: &[u8],
) {
	state.push_ref(ResolvedRef {
		source: site.source.clone(),
		target,
		kind: kinds::CALLS,
		position: Some(site.position),
		confidence,
		hints: call_hints(site.name, site.arity, hint),
	});
}

fn call_hints(name: &[u8], arity: usize, hint: &[u8]) -> RefHints {
	RefHints {
		receiver_hint: hint.to_vec(),
		call_name: name.to_vec(),
		call_arity: Some(arity),
		..RefHints::default()
	}
}

fn module_declaration_refs(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	specifier_type_refs_of(state, node, scope);
	if let Some(ty) = node.child_by_field_name("type") {
		emit_uses_type(state, ty, scope);
	}
	prototype_param_refs(state, node, scope);
	let mut env = TypeEnv::default();
	let mut cursor = node.walk();
	for declarator in node.children_by_field_name("declarator", &mut cursor) {
		if declarator.kind() == "init_declarator"
			&& let Some(value) = declarator.child_by_field_name("value")
		{
			expr_refs(state, value, scope, &mut env);
		}
	}
}

fn prototype_param_refs(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let mut cursor = node.walk();
	for declarator in node.children_by_field_name("declarator", &mut cursor) {
		let Some(info) = declarator_info(declarator, state.source) else {
			continue;
		};
		if !info.is_function {
			continue;
		}
		let slots = info
			.parameters
			.map(|params| parameter_slots(params, state.source))
			.unwrap_or_default();
		let callable =
			crate::lang::callable::extend_callable_slots(scope, kinds::FUNC, info.name, &slots);
		if let Some(params) = info.parameters {
			for param in named_children(params) {
				if let Some(ty) = param.child_by_field_name("type") {
					emit_uses_type(state, ty, &callable);
				}
			}
		}
		if let Some(return_type) = node.child_by_field_name("type") {
			push_returns_type(state, &callable, return_type, info.pointer_depth);
		}
	}
}

fn typedef_refs(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	specifier_type_refs_of(state, node, scope);
	let Some(specifier) = node.child_by_field_name("type") else {
		return;
	};
	if specifier.child_by_field_name("body").is_some() {
		return;
	}
	for declarator in {
		let mut cursor = node.walk();
		node.children_by_field_name("declarator", &mut cursor)
			.collect::<Vec<_>>()
	} {
		let Some(info) = declarator_info(declarator, state.source) else {
			continue;
		};
		let alias = extend_segment(scope, kinds::TYPE, info.name);
		let aliased_name = specifier
			.child_by_field_name("name")
			.map(|name_node| node_slice(name_node, state.source));
		if aliased_name == Some(info.name) && state.type_table.get(info.name) == Some(&alias) {
			continue;
		}
		emit_uses_type(state, specifier, &alias);
	}
}

fn specifier_type_refs_of(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	if let Some(specifier) = node.child_by_field_name("type") {
		specifier_type_refs_for(state, specifier, scope);
	}
}

// Field type usages hang off the owning struct so member declarations link
// their types even when the struct is only reachable through a typedef.
fn specifier_type_refs_for(state: &mut CDiscover<'_>, specifier: Node<'_>, scope: &Moniker) {
	if !matches!(specifier.kind(), "struct_specifier" | "union_specifier") {
		return;
	}
	let Some(body) = specifier.child_by_field_name("body") else {
		return;
	};
	let owner = specifier
		.child_by_field_name("name")
		.map(|name_node| extend_segment(scope, kinds::STRUCT, node_slice(name_node, state.source)))
		.unwrap_or_else(|| scope.clone());
	for field in named_children(body) {
		if field.kind() != "field_declaration" {
			continue;
		}
		if let Some(ty) = field.child_by_field_name("type") {
			emit_uses_type(state, ty, &owner);
		}
	}
}

pub(super) fn emit_uses_type(state: &mut CDiscover<'_>, type_node: Node<'_>, source: &Moniker) {
	let resolved = match type_node.kind() {
		"type_identifier" => {
			let name = node_slice(type_node, state.source);
			if name.is_empty() || is_c_primitive(name) {
				return;
			}
			Some(resolve_type_target(state, name, kinds::TYPE))
		}
		"struct_specifier" | "union_specifier" | "enum_specifier" => {
			let fallback = if type_node.kind() == "enum_specifier" {
				kinds::ENUM
			} else {
				kinds::STRUCT
			};
			type_node.child_by_field_name("name").map(|name_node| {
				resolve_type_target(state, node_slice(name_node, state.source), fallback)
			})
		}
		"sized_type_specifier" | "primitive_type" => None,
		_ => {
			for child in named_children(type_node) {
				emit_uses_type(state, child, source);
			}
			None
		}
	};
	let Some((target, confidence)) = resolved else {
		return;
	};
	state.push_ref(ResolvedRef {
		source: source.clone(),
		target,
		kind: kinds::USES_TYPE,
		position: Some(node_position(type_node)),
		confidence,
		hints: RefHints::default(),
	});
}
