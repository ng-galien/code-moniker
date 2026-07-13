use tree_sitter::Node;

use crate::core::moniker::Moniker;
use crate::lang::callable::{CallableSlot, extend_callable_slots, extend_segment};
use crate::lang::sdk::{RefHints, ResolvedRef, TypeEnv, TypeExpr};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::builtins::{builtin_func_target, is_builtin_func};
use super::discover::GoDiscover;
use super::syntax::{
	argument_count, named_children, receiver_hint_bytes, receiver_type_name, spec_children,
	struct_field_list,
};
use super::type_resolution::{
	external_target_shape, lookup_import, owner_confidence, resolve_type_node, type_expr,
};

pub(super) fn collect_refs(state: &mut GoDiscover<'_>, root: Node<'_>, scope: &Moniker) {
	for child in named_children(root) {
		match child.kind() {
			"type_declaration" => {
				for spec in named_children(child) {
					type_spec_refs(state, spec, scope);
				}
			}
			"function_declaration" => callable_refs(state, child, scope, None),
			"method_declaration" => method_refs(state, child, scope),
			"var_declaration" => module_value_refs(state, child, scope, "var_spec"),
			"const_declaration" => module_value_refs(state, child, scope, "const_spec"),
			_ => {}
		}
	}
}

fn type_spec_refs(state: &mut GoDiscover<'_>, spec: Node<'_>, scope: &Moniker) {
	let (kind, name_node, type_node) = match spec.kind() {
		"type_spec" => (
			super::defs::type_spec_kind(spec),
			spec.child_by_field_name("name"),
			spec.child_by_field_name("type"),
		),
		"type_alias" => (
			kinds::TYPE,
			spec.child_by_field_name("name"),
			spec.child_by_field_name("type"),
		),
		_ => return,
	};
	let (Some(name_node), Some(type_node)) = (name_node, type_node) else {
		return;
	};
	let owner = extend_segment(scope, kind, node_slice(name_node, state.source));
	match type_node.kind() {
		"struct_type" => struct_body_refs(state, type_node, &owner),
		"interface_type" => interface_body_refs(state, type_node, &owner),
		_ => emit_uses_type(state, type_node, &owner),
	}
}

fn struct_body_refs(state: &mut GoDiscover<'_>, struct_node: Node<'_>, owner: &Moniker) {
	let Some(field_list) = struct_field_list(struct_node) else {
		return;
	};
	for field in named_children(field_list) {
		if field.kind() != "field_declaration" {
			continue;
		}
		let Some(type_node) = field.child_by_field_name("type") else {
			continue;
		};
		if field.child_by_field_name("name").is_some() {
			emit_uses_type(state, type_node, owner);
		} else {
			emit_extends(state, type_node, owner);
		}
	}
}

fn interface_body_refs(state: &mut GoDiscover<'_>, interface_node: Node<'_>, owner: &Moniker) {
	for child in named_children(interface_node) {
		match child.kind() {
			"method_elem" => {
				if let Some(params) = child.child_by_field_name("parameters") {
					for param in named_children(params) {
						if let Some(ty) = param.child_by_field_name("type") {
							emit_uses_type(state, ty, owner);
						}
					}
				}
				if let Some(result) = child.child_by_field_name("result") {
					emit_uses_type(state, result, owner);
				}
			}
			"type_elem" => {
				for ty in named_children(child) {
					emit_extends(state, ty, owner);
				}
			}
			_ => {}
		}
	}
}

fn method_refs(state: &mut GoDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let owner = node
		.child_by_field_name("receiver")
		.and_then(|receiver| receiver_type_name(receiver, state.source))
		.map(|receiver_name| {
			state
				.type_table
				.get(receiver_name)
				.cloned()
				.unwrap_or_else(|| extend_segment(scope, kinds::STRUCT, receiver_name))
		});
	callable_refs(state, node, scope, owner);
}

fn callable_refs(
	state: &mut GoDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	owner_type: Option<Moniker>,
) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	let slots = super::syntax::function_param_slots(node, state.source);
	let kind = if owner_type.is_some() {
		kinds::METHOD
	} else {
		kinds::FUNC
	};
	let parent = owner_type.clone().unwrap_or_else(|| scope.clone());
	let callable = extend_callable_slots(&parent, kind, name, &slots);

	if let Some(params) = node.child_by_field_name("parameters") {
		for param in named_children(params) {
			if let Some(ty) = param.child_by_field_name("type") {
				emit_uses_type(state, ty, &callable);
			}
		}
	}
	if let Some(result) = node.child_by_field_name("result") {
		emit_uses_type(state, result, &callable);
	}

	let Some(body) = node.child_by_field_name("body") else {
		return;
	};
	let mut env = TypeEnv::default();
	bind_receiver(state, node, owner_type.as_ref(), &mut env);
	bind_params(state, node, &mut env);
	collect_local_types(state, body, owner_type.as_ref(), &mut env);
	expr_refs(state, body, &callable, owner_type.as_ref(), &env);
}

fn bind_receiver(
	state: &GoDiscover<'_>,
	node: Node<'_>,
	owner_type: Option<&Moniker>,
	env: &mut TypeEnv,
) {
	let (Some(receiver), Some(owner)) = (node.child_by_field_name("receiver"), owner_type) else {
		return;
	};
	for param in named_children(receiver) {
		if param.kind() != "parameter_declaration" {
			continue;
		}
		for name_node in named_children(param).filter(|child| child.kind() == "identifier") {
			let name = node_slice(name_node, state.source);
			if !name.is_empty() && name != b"_" {
				env.bind_local(name, TypeExpr::resolved(owner.clone()));
			}
		}
	}
}

fn bind_params(state: &GoDiscover<'_>, node: Node<'_>, env: &mut TypeEnv) {
	let Some(params) = node.child_by_field_name("parameters") else {
		return;
	};
	bind_param_list(state, params, env);
}

fn bind_param_list(state: &GoDiscover<'_>, params: Node<'_>, env: &mut TypeEnv) {
	for param in named_children(params) {
		if !matches!(
			param.kind(),
			"parameter_declaration" | "variadic_parameter_declaration"
		) {
			continue;
		}
		let ty = param
			.child_by_field_name("type")
			.and_then(|node| type_expr(state, node));
		for name_node in named_children(param).filter(|child| child.kind() == "identifier") {
			let name = node_slice(name_node, state.source);
			if name.is_empty() || name == b"_" {
				continue;
			}
			env.bind_local(name, ty.clone().unwrap_or(TypeExpr::Unknown));
		}
	}
}

fn collect_local_types(
	state: &GoDiscover<'_>,
	node: Node<'_>,
	owner_type: Option<&Moniker>,
	env: &mut TypeEnv,
) {
	match node.kind() {
		"short_var_declaration" => {
			bind_short_var(state, node, owner_type, env);
		}
		"var_declaration" | "const_declaration" => {
			let spec_kind = if node.kind() == "var_declaration" {
				"var_spec"
			} else {
				"const_spec"
			};
			for spec in spec_children(node, spec_kind) {
				let declared = spec
					.child_by_field_name("type")
					.and_then(|ty| type_expr(state, ty));
				let inferred = spec
					.child_by_field_name("value")
					.and_then(|value| named_children(value).next())
					.and_then(|expr| infer_value_type(state, expr, owner_type, env));
				let ty = declared.or(inferred).unwrap_or(TypeExpr::Unknown);
				for name_node in named_children(spec).filter(|child| child.kind() == "identifier") {
					let name = node_slice(name_node, state.source);
					if !name.is_empty() && name != b"_" {
						env.bind_local(name, ty.clone());
					}
				}
			}
		}
		"range_clause" => {
			if let Some(left) = node.child_by_field_name("left") {
				bind_unknown_ids(state, left, env);
			}
		}
		_ => {}
	}
	for child in named_children(node) {
		collect_local_types(state, child, owner_type, env);
	}
}

fn bind_short_var(
	state: &GoDiscover<'_>,
	node: Node<'_>,
	owner_type: Option<&Moniker>,
	env: &mut TypeEnv,
) {
	let (Some(left), Some(right)) = (
		node.child_by_field_name("left"),
		node.child_by_field_name("right"),
	) else {
		return;
	};
	let names = expression_list_ids(state, left);
	let values = expression_list_exprs(right);
	for (index, name) in names.iter().enumerate() {
		let value = if values.len() == names.len() {
			values.get(index).copied()
		} else if index == 0 {
			values.first().copied()
		} else {
			None
		};
		let ty = value
			.and_then(|expr| infer_value_type(state, expr, owner_type, env))
			.unwrap_or(TypeExpr::Unknown);
		env.bind_local(name.clone(), ty);
	}
}

fn bind_unknown_ids(state: &GoDiscover<'_>, node: Node<'_>, env: &mut TypeEnv) {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, state.source);
			if !name.is_empty() && name != b"_" {
				env.bind_local(name, TypeExpr::Unknown);
			}
		}
		"expression_list" => {
			for child in named_children(node) {
				bind_unknown_ids(state, child, env);
			}
		}
		_ => {}
	}
}

fn expression_list_ids(state: &GoDiscover<'_>, node: Node<'_>) -> Vec<Vec<u8>> {
	let mut out = Vec::new();
	let mut push = |node: Node<'_>| {
		let name = node_slice(node, state.source);
		if node.kind() == "identifier" && !name.is_empty() && name != b"_" {
			out.push(name.to_vec());
		}
	};
	if node.kind() == "expression_list" {
		for child in named_children(node) {
			push(child);
		}
	} else {
		push(node);
	}
	out
}

fn expression_list_exprs(node: Node<'_>) -> Vec<Node<'_>> {
	if node.kind() == "expression_list" {
		named_children(node).collect()
	} else {
		vec![node]
	}
}

fn infer_value_type(
	state: &GoDiscover<'_>,
	value: Node<'_>,
	owner_type: Option<&Moniker>,
	env: &TypeEnv,
) -> Option<TypeExpr> {
	match value.kind() {
		"call_expression" => infer_call_type(state, value, owner_type, env),
		"composite_literal" => value
			.child_by_field_name("type")
			.and_then(|ty| type_expr(state, ty)),
		"unary_expression" => value
			.child_by_field_name("operand")
			.and_then(|inner| infer_value_type(state, inner, owner_type, env)),
		"identifier" => env.resolve_local(node_slice(value, state.source)).cloned(),
		"type_assertion_expression" => value
			.child_by_field_name("type")
			.and_then(|ty| type_expr(state, ty)),
		_ => None,
	}
}

fn infer_call_type(
	state: &GoDiscover<'_>,
	call: Node<'_>,
	owner_type: Option<&Moniker>,
	env: &TypeEnv,
) -> Option<TypeExpr> {
	let callee = call.child_by_field_name("function")?;
	match callee.kind() {
		"identifier" => {
			let name = node_slice(callee, state.source);
			if let Some(target) = state.type_table.get(name) {
				return Some(TypeExpr::resolved(target.clone()));
			}
			state
				.return_types
				.get(&(state.root.clone(), name.to_vec()))
				.cloned()
		}
		"selector_expression" => {
			let name_node = callee.child_by_field_name("field")?;
			let name = node_slice(name_node, state.source);
			let operand = callee.child_by_field_name("operand")?;
			if operand.kind() == "identifier"
				&& let Some(entry) = lookup_import(state, node_slice(operand, state.source))
			{
				return Some(TypeExpr::external_opaque(extend_segment(
					&entry.target,
					kinds::FUNC,
					name,
				)));
			}
			let receiver = receiver_type_expr(state, operand, owner_type, env)?;
			let owner = receiver.receiver_owner()?;
			state
				.return_types
				.get(&(owner.clone(), name.to_vec()))
				.cloned()
				.or_else(|| {
					external_target_shape(owner).then(|| {
						TypeExpr::external_opaque(extend_arity_call(
							owner,
							kinds::METHOD,
							name,
							argument_count_of(call),
						))
					})
				})
		}
		_ => None,
	}
}

fn receiver_type_expr(
	state: &GoDiscover<'_>,
	node: Node<'_>,
	owner_type: Option<&Moniker>,
	env: &TypeEnv,
) -> Option<TypeExpr> {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, state.source);
			env.resolve_local(name).cloned().or_else(|| {
				state
					.type_table
					.get(name)
					.map(|target| TypeExpr::resolved(target.clone()))
			})
		}
		"selector_expression" => {
			let field = node.child_by_field_name("field")?;
			let field_name = node_slice(field, state.source);
			let operand = node.child_by_field_name("operand")?;
			if operand.kind() == "identifier"
				&& let Some(entry) = lookup_import(state, node_slice(operand, state.source))
			{
				return Some(TypeExpr::external_opaque(extend_segment(
					&entry.target,
					kinds::PATH,
					field_name,
				)));
			}
			let receiver = receiver_type_expr(state, operand, owner_type, env)?;
			let owner = receiver.receiver_owner()?;
			state
				.field_types
				.get(&(owner.clone(), field_name.to_vec()))
				.cloned()
				.or_else(|| {
					external_target_shape(owner).then(|| {
						TypeExpr::external_opaque(extend_segment(owner, kinds::PATH, field_name))
					})
				})
		}
		"call_expression" => infer_call_type(state, node, owner_type, env),
		"unary_expression" => node
			.child_by_field_name("operand")
			.and_then(|inner| receiver_type_expr(state, inner, owner_type, env)),
		"parenthesized_expression" => named_children(node)
			.next()
			.and_then(|inner| receiver_type_expr(state, inner, owner_type, env)),
		"composite_literal" => node
			.child_by_field_name("type")
			.and_then(|ty| type_expr(state, ty)),
		"type_assertion_expression" => node
			.child_by_field_name("type")
			.and_then(|ty| type_expr(state, ty)),
		_ => None,
	}
}

fn expr_refs(
	state: &mut GoDiscover<'_>,
	node: Node<'_>,
	source: &Moniker,
	owner_type: Option<&Moniker>,
	env: &TypeEnv,
) {
	match node.kind() {
		"call_expression" => {
			call_ref(state, node, source, owner_type, env);
			return;
		}
		"composite_literal" => {
			if let Some(type_node) = node.child_by_field_name("type") {
				emit_instantiates(state, type_node, source, node_position(node));
			}
			if let Some(body) = node.child_by_field_name("body") {
				expr_refs(state, body, source, owner_type, env);
			}
			return;
		}
		"func_literal" => {
			let mut literal_env = env.clone();
			if let Some(params) = node.child_by_field_name("parameters") {
				bind_param_list(state, params, &mut literal_env);
			}
			if let Some(body) = node.child_by_field_name("body") {
				expr_refs(state, body, source, owner_type, &literal_env);
			}
			return;
		}
		"short_var_declaration" | "range_clause" => {
			if let Some(right) = node.child_by_field_name("right") {
				expr_refs(state, right, source, owner_type, env);
			}
			return;
		}
		"var_declaration" | "const_declaration" => {
			let spec_kind = if node.kind() == "var_declaration" {
				"var_spec"
			} else {
				"const_spec"
			};
			for spec in spec_children(node, spec_kind) {
				if let Some(ty) = spec.child_by_field_name("type") {
					emit_uses_type(state, ty, source);
				}
				if let Some(value) = spec.child_by_field_name("value") {
					expr_refs(state, value, source, owner_type, env);
				}
			}
			return;
		}
		_ => {}
	}
	for child in named_children(node) {
		expr_refs(state, child, source, owner_type, env);
	}
}

#[derive(Clone, Copy)]
struct CallSite<'a> {
	source: &'a Moniker,
	position: (u32, u32),
	name: &'a [u8],
	arity: usize,
}

fn call_ref(
	state: &mut GoDiscover<'_>,
	call: Node<'_>,
	source: &Moniker,
	owner_type: Option<&Moniker>,
	env: &TypeEnv,
) {
	let site = CallSite {
		source,
		position: node_position(call),
		name: b"",
		arity: argument_count_of(call),
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
			"selector_expression" => {
				selector_call_ref(state, callee, site, owner_type, env);
			}
			_ => expr_refs(state, callee, source, owner_type, env),
		}
	}
	if let Some(arguments) = call.child_by_field_name("arguments") {
		expr_refs(state, arguments, source, owner_type, env);
	}
}

fn simple_call_ref(state: &mut GoDiscover<'_>, site: &CallSite<'_>, env: &TypeEnv) {
	let name = site.name;
	if name.is_empty() {
		return;
	}
	if let Some(entry) = lookup_import(state, name).cloned() {
		let target = extend_segment(&entry.target, kinds::FUNC, name);
		push_call(state, site, target, entry.confidence);
		return;
	}
	if let Some(target) = state.type_table.get(name).cloned() {
		state.push_ref(ResolvedRef {
			source: site.source.clone(),
			target,
			kind: kinds::USES_TYPE,
			position: Some(site.position),
			confidence: kinds::CONF_RESOLVED,
			hints: RefHints::default(),
		});
		return;
	}
	if env.resolve_local(name).is_some() {
		if state.deep {
			let target = extend_segment(site.source, kinds::LOCAL, name);
			push_call_with(state, site, target, kinds::CONF_LOCAL);
		}
		return;
	}
	if let Some(segment) = state.callables.get(&(state.root.clone(), name.to_vec())) {
		let target = extend_segment(&state.root, kinds::FUNC, segment);
		push_call(state, site, target, kinds::CONF_RESOLVED);
		return;
	}
	if is_builtin_func(name) {
		let target = builtin_func_target(&state.root, name);
		push_call(state, site, target, kinds::CONF_EXTERNAL);
		return;
	}
	let target = extend_segment(&state.root, kinds::FUNC, name);
	push_call(state, site, target, kinds::CONF_NAME_MATCH);
}

fn selector_call_ref(
	state: &mut GoDiscover<'_>,
	callee: Node<'_>,
	site: CallSite<'_>,
	owner_type: Option<&Moniker>,
	env: &TypeEnv,
) {
	let Some(field) = callee.child_by_field_name("field") else {
		return;
	};
	let name = node_slice(field, state.source);
	if name.is_empty() {
		return;
	}
	let site = CallSite { name, ..site };
	let operand = callee.child_by_field_name("operand");

	if let Some(op) = operand
		&& op.kind() == "identifier"
		&& let Some(entry) = lookup_import(state, node_slice(op, state.source)).cloned()
	{
		let target = extend_segment(&entry.target, kinds::FUNC, name);
		push_call(state, &site, target, entry.confidence);
		return;
	}

	let hint = operand
		.map(|op| receiver_hint_bytes(op, state.source))
		.unwrap_or(b"");
	let receiver_owner = operand
		.and_then(|op| receiver_type_expr(state, op, owner_type, env))
		.and_then(|ty| ty.receiver_owner().cloned());
	let (target, confidence) = match &receiver_owner {
		Some(owner) => {
			let target = state
				.callables
				.get(&(owner.clone(), name.to_vec()))
				.map(|segment| extend_segment(owner, kinds::METHOD, segment))
				.unwrap_or_else(|| extend_arity_call(owner, kinds::METHOD, name, site.arity));
			(target, owner_confidence(state, owner))
		}
		None => (
			extend_segment(&state.root, kinds::METHOD, name),
			kinds::CONF_NAME_MATCH,
		),
	};
	state.push_ref(ResolvedRef {
		source: site.source.clone(),
		target,
		kind: kinds::METHOD_CALL,
		position: Some(site.position),
		confidence,
		hints: call_hints(name, site.arity, hint),
	});

	if let Some(op) = operand {
		expr_refs(state, op, site.source, owner_type, env);
	}
}

fn push_call(
	state: &mut GoDiscover<'_>,
	site: &CallSite<'_>,
	target: Moniker,
	confidence: &'static [u8],
) {
	push_call_with(state, site, target, confidence);
}

fn push_call_with(
	state: &mut GoDiscover<'_>,
	site: &CallSite<'_>,
	target: Moniker,
	confidence: &'static [u8],
) {
	state.push_ref(ResolvedRef {
		source: site.source.clone(),
		target,
		kind: kinds::CALLS,
		position: Some(site.position),
		confidence,
		hints: call_hints(site.name, site.arity, b""),
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

fn argument_count_of(call: Node<'_>) -> usize {
	call.child_by_field_name("arguments")
		.map(argument_count)
		.unwrap_or(0)
}

fn extend_arity_call(parent: &Moniker, kind: &[u8], name: &[u8], arity: usize) -> Moniker {
	let slots = vec![CallableSlot::default(); arity];
	extend_callable_slots(parent, kind, name, &slots)
}

fn emit_instantiates(
	state: &mut GoDiscover<'_>,
	type_node: Node<'_>,
	source: &Moniker,
	position: (u32, u32),
) {
	match type_node.kind() {
		"type_identifier" | "qualified_type" => {
			if let Some((target, confidence)) = resolve_type_node(state, type_node) {
				state.push_ref(ResolvedRef {
					source: source.clone(),
					target,
					kind: kinds::INSTANTIATES,
					position: Some(position),
					confidence,
					hints: RefHints::default(),
				});
			}
		}
		"generic_type" => {
			if let Some(inner) = type_node.child_by_field_name("type") {
				emit_instantiates(state, inner, source, position);
			}
		}
		_ => {}
	}
}

pub(super) fn emit_uses_type(state: &mut GoDiscover<'_>, type_node: Node<'_>, source: &Moniker) {
	match type_node.kind() {
		"type_identifier" | "qualified_type" => {
			if let Some((target, confidence)) = resolve_type_node(state, type_node) {
				state.push_ref(ResolvedRef {
					source: source.clone(),
					target,
					kind: kinds::USES_TYPE,
					position: Some(node_position(type_node)),
					confidence,
					hints: RefHints::default(),
				});
			}
		}
		"pointer_type" | "slice_type" | "array_type" | "channel_type" | "map_type"
		| "parenthesized_type" => {
			for child in named_children(type_node) {
				emit_uses_type(state, child, source);
			}
		}
		"generic_type" => {
			if let Some(head) = type_node.child_by_field_name("type") {
				emit_uses_type(state, head, source);
			}
			if let Some(args) = type_node.child_by_field_name("type_arguments") {
				for child in named_children(args) {
					emit_uses_type(state, child, source);
				}
			}
		}
		"function_type" => {
			if let Some(params) = type_node.child_by_field_name("parameters") {
				for param in named_children(params) {
					if let Some(ty) = param.child_by_field_name("type") {
						emit_uses_type(state, ty, source);
					}
				}
			}
			if let Some(result) = type_node.child_by_field_name("result") {
				emit_uses_type(state, result, source);
			}
		}
		"parameter_list" => {
			for param in named_children(type_node) {
				if let Some(ty) = param.child_by_field_name("type") {
					emit_uses_type(state, ty, source);
				}
			}
		}
		_ => {}
	}
}

fn emit_extends(state: &mut GoDiscover<'_>, type_node: Node<'_>, owner: &Moniker) {
	match type_node.kind() {
		"type_identifier" | "qualified_type" => {
			if let Some((target, confidence)) = resolve_type_node(state, type_node) {
				state.push_ref(ResolvedRef {
					source: owner.clone(),
					target,
					kind: kinds::EXTENDS,
					position: Some(node_position(type_node)),
					confidence,
					hints: RefHints::default(),
				});
			}
		}
		"pointer_type" => {
			for child in named_children(type_node) {
				emit_extends(state, child, owner);
			}
		}
		"generic_type" => {
			if let Some(head) = type_node.child_by_field_name("type") {
				emit_extends(state, head, owner);
			}
		}
		_ => {}
	}
}

fn module_value_refs(state: &mut GoDiscover<'_>, node: Node<'_>, scope: &Moniker, spec_kind: &str) {
	let env = TypeEnv::default();
	for spec in spec_children(node, spec_kind) {
		if let Some(ty) = spec.child_by_field_name("type") {
			emit_uses_type(state, ty, scope);
		}
		if let Some(value) = spec.child_by_field_name("value") {
			expr_refs(state, value, scope, None, &env);
		}
	}
}
