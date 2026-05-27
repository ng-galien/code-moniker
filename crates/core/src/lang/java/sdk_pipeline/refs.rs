use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::callable::{CallableSlot, extend_callable_slots, extend_segment};
use crate::lang::sdk::{RefHints, ResolvedRef, TypeEnv, TypeExpr};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::{builtins, kinds};
use super::defs::formal_parameter_slots;
use super::discover::JavaDiscover;
use super::imports::{java_external_target_shape, java_lang_target, same_package_symbol_target};
use super::syntax::{last_identifier, named_children, type_name};

pub(super) fn collect_refs(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	for child in named_children(node) {
		match child.kind() {
			"class_declaration" => type_refs(state, child, scope, kinds::CLASS),
			"interface_declaration" => type_refs(state, child, scope, kinds::INTERFACE),
			"enum_declaration" => type_refs(state, child, scope, kinds::ENUM),
			"record_declaration" => type_refs(state, child, scope, kinds::RECORD),
			"annotation_type_declaration" => type_refs(state, child, scope, kinds::ANNOTATION_TYPE),
			"method_declaration" | "constructor_declaration" => callable_refs(state, child, scope),
			"field_declaration" => field_refs(state, child, scope),
			"import_declaration" | "package_declaration" => {}
			_ => collect_refs(state, child, scope),
		}
	}
}

fn type_refs(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker, kind: &'static [u8]) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let type_scope = extend_segment(scope, kind, node_slice(name_node, state.source));
	annotations(state, node, &type_scope);
	for child in named_children(node) {
		match child.kind() {
			"superclass" => heritage_refs(state, child, &type_scope, kinds::EXTENDS),
			"super_interfaces" | "extends_interfaces" => {
				heritage_refs(state, child, &type_scope, kinds::IMPLEMENTS)
			}
			_ => {}
		}
	}
	if let Some(body) = node.child_by_field_name("body") {
		collect_refs(state, body, &type_scope);
	}
}

fn callable_refs(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let kind = if node.kind() == "constructor_declaration" {
		kinds::CONSTRUCTOR
	} else {
		kinds::METHOD
	};
	let slots = formal_parameter_slots(node, state.source);
	let callable = extend_callable_slots(scope, kind, node_slice(name_node, state.source), &slots);
	annotations(state, node, &callable);
	if let Some(ty) = node.child_by_field_name("type") {
		emit_type_refs(state, ty, &callable);
	}
	if let Some(params) = node.child_by_field_name("parameters") {
		for param in named_children(params) {
			if let Some(ty) = param.child_by_field_name("type") {
				emit_type_refs(state, ty, &callable);
			}
			if state.deep {
				param_annotations(state, param, &callable);
			}
		}
	}
	if let Some(body) = node.child_by_field_name("body") {
		let type_env = callable_type_env(state, node, body, scope);
		expr_refs(state, body, &callable, scope, &type_env);
	}
}

fn field_refs(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	if let Some(ty) = node.child_by_field_name("type") {
		emit_type_refs(state, ty, scope);
	}
	for declarator in named_children(node).filter(|child| child.kind() == "variable_declarator") {
		if let Some(value) = declarator.child_by_field_name("value") {
			expr_refs(state, value, scope, scope, &TypeEnv::default());
		}
	}
}

fn callable_type_env(
	state: &JavaDiscover<'_>,
	callable: Node<'_>,
	body: Node<'_>,
	owner: &Moniker,
) -> TypeEnv {
	let mut env = TypeEnv::default();
	if let Some(params) = callable.child_by_field_name("parameters") {
		for param in named_children(params) {
			let Some(name_node) = param.child_by_field_name("name") else {
				continue;
			};
			let Some(ty) = param
				.child_by_field_name("type")
				.and_then(|node| type_expr(state, node))
			else {
				continue;
			};
			env.bind_local(node_slice(name_node, state.source), ty);
		}
	}
	collect_local_types(state, body, owner, &mut env);
	env
}

fn collect_local_types(
	state: &JavaDiscover<'_>,
	node: Node<'_>,
	owner: &Moniker,
	env: &mut TypeEnv,
) {
	if node.kind() == "local_variable_declaration" {
		let type_node = node.child_by_field_name("type");
		let declared_type = type_node.and_then(|node| type_expr(state, node));
		let infer_var = type_node
			.and_then(|node| type_name(node, state.source))
			.is_some_and(|name| builtins::is_inferred_local_type(&name));
		for declarator in named_children(node).filter(|child| child.kind() == "variable_declarator")
		{
			let Some(name_node) = declarator.child_by_field_name("name") else {
				continue;
			};
			let ty = if infer_var {
				declarator
					.child_by_field_name("value")
					.and_then(|value| infer_value_type(state, value, owner, env))
			} else {
				declared_type.clone()
			};
			if let Some(ty) = ty {
				env.bind_local(node_slice(name_node, state.source), ty);
			}
		}
	}
	for child in named_children(node) {
		collect_local_types(state, child, owner, env);
	}
}

fn expr_refs(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	source: &Moniker,
	owner: &Moniker,
	env: &TypeEnv,
) {
	match node.kind() {
		"method_invocation" => {
			method_call_ref(state, node, source, owner, env);
			if let Some(object) = node.child_by_field_name("object") {
				expr_refs(state, object, source, owner, env);
			}
			if let Some(arguments) = node.child_by_field_name("arguments") {
				for arg in named_children(arguments) {
					expr_refs(state, arg, source, owner, env);
				}
			}
			return;
		}
		"object_creation_expression" => object_creation_ref(state, node, source),
		"local_variable_declaration" => {
			if let Some(ty) = node.child_by_field_name("type") {
				emit_type_refs(state, ty, source);
			}
		}
		"identifier" => {
			identifier_ref(state, node, source, owner, env);
			return;
		}
		"type_identifier" | "scoped_type_identifier" | "generic_type" | "array_type" => {
			emit_type_refs(state, node, source);
			return;
		}
		_ => {}
	}
	for child in named_children(node) {
		expr_refs(state, child, source, owner, env);
	}
}

fn method_call_ref(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	source: &Moniker,
	owner: &Moniker,
	env: &TypeEnv,
) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	let arity = node
		.child_by_field_name("arguments")
		.map(argument_count)
		.unwrap_or(0);
	let (target, confidence, receiver_hint) =
		if let Some(object) = node.child_by_field_name("object") {
			let receiver_owner = receiver_owner(state, object, owner, env);
			let target = receiver_owner
				.as_ref()
				.map(|owner| method_target(state, owner, name, arity))
				.unwrap_or_else(|| extend_arity_call(&state.root, kinds::METHOD, name, arity));
			let confidence = receiver_owner
				.as_ref()
				.map(|owner| owner_confidence(state, owner))
				.unwrap_or(kinds::CONF_NAME_MATCH);
			(
				target,
				confidence,
				receiver_hint_bytes(object, state.source).to_vec(),
			)
		} else if let Some(target) = lookup_callable(state, owner, name, arity) {
			(target, kinds::CONF_RESOLVED, Vec::new())
		} else {
			(
				extend_arity_call(owner, kinds::METHOD, name, arity),
				kinds::CONF_NAME_MATCH,
				Vec::new(),
			)
		};
	let mut hints = RefHints::default();
	hints.receiver_hint = receiver_hint;
	hints.call_name = name.to_vec();
	hints.call_arity = Some(arity);
	state.push_ref(ResolvedRef {
		source: source.clone(),
		target,
		kind: if node.child_by_field_name("object").is_some() {
			kinds::METHOD_CALL
		} else {
			kinds::CALLS
		},
		position: Some(node_position(node)),
		confidence,
		hints,
	});
}

fn object_creation_ref(state: &mut JavaDiscover<'_>, node: Node<'_>, source: &Moniker) {
	let Some(ty) = node.child_by_field_name("type") else {
		return;
	};
	let Some(name) = type_name(ty, state.source) else {
		return;
	};
	let (target, confidence) = resolve_type_target(state, &name, kinds::CLASS);
	state.push_ref(ResolvedRef {
		source: source.clone(),
		target,
		kind: kinds::INSTANTIATES,
		position: Some(node_position(node)),
		confidence,
		hints: RefHints::default(),
	});
}

fn identifier_ref(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	source: &Moniker,
	owner: &Moniker,
	env: &TypeEnv,
) {
	let name = node_slice(node, state.source);
	if name.is_empty() {
		return;
	}
	if env.resolve_local(name).is_some() {
		state.push_ref(ResolvedRef {
			source: source.clone(),
			target: extend_segment(source, kinds::LOCAL, name),
			kind: kinds::READS,
			position: Some(node_position(node)),
			confidence: kinds::CONF_LOCAL,
			hints: RefHints::default(),
		});
		return;
	}
	if let Some(cls) = enclosing_type(owner)
		&& state
			.field_types
			.contains_key(&(cls.clone(), name.to_vec()))
	{
		state.push_ref(ResolvedRef {
			source: source.clone(),
			target: extend_segment(&cls, kinds::FIELD, name),
			kind: kinds::READS,
			position: Some(node_position(node)),
			confidence: kinds::CONF_RESOLVED,
			hints: RefHints::default(),
		});
	}
}

fn receiver_owner(
	state: &JavaDiscover<'_>,
	receiver: Node<'_>,
	owner: &Moniker,
	env: &TypeEnv,
) -> Option<Moniker> {
	match receiver.kind() {
		"this" | "super" => enclosing_type(owner),
		"identifier" => {
			let name = node_slice(receiver, state.source);
			env.resolve_local(name)
				.and_then(TypeExpr::receiver_owner)
				.cloned()
				.or_else(|| {
					enclosing_type(owner)
						.and_then(|cls| state.field_types.get(&(cls, name.to_vec())).cloned())
						.and_then(|ty| ty.receiver_owner().cloned())
				})
				.or_else(|| lookup_known_type_name(state, name).map(|(target, _)| target))
		}
		"object_creation_expression" => receiver
			.child_by_field_name("type")
			.and_then(|ty| type_expr(state, ty))
			.and_then(|ty| ty.receiver_owner().cloned()),
		"field_access" | "scoped_identifier" => {
			expression_external_owner(state, receiver, owner, env)
		}
		"method_invocation" => {
			infer_call_type(state, receiver, owner, env).and_then(|ty| ty.receiver_owner().cloned())
		}
		"string_literal" => Some(java_lang_target(&state.root, b"String")),
		_ => None,
	}
}

fn expression_external_owner(
	state: &JavaDiscover<'_>,
	node: Node<'_>,
	owner: &Moniker,
	env: &TypeEnv,
) -> Option<Moniker> {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, state.source);
			env.resolve_local(name)
				.and_then(TypeExpr::receiver_owner)
				.filter(|target| java_external_target_shape(target))
				.cloned()
				.or_else(|| match lookup_known_type_name(state, name) {
					Some((target, kinds::CONF_EXTERNAL)) => Some(target),
					_ => None,
				})
		}
		"string_literal" => Some(java_lang_target(&state.root, b"String")),
		"type_identifier" | "scoped_type_identifier" | "generic_type" | "array_type" => {
			let name = type_name(node, state.source)?;
			let (target, confidence) = resolve_type_target(state, &name, kinds::CLASS);
			(confidence == kinds::CONF_EXTERNAL).then_some(target)
		}
		"field_access" | "scoped_identifier" => node
			.child_by_field_name("object")
			.and_then(|object| expression_external_owner(state, object, owner, env)),
		"method_invocation" => infer_call_type(state, node, owner, env)
			.and_then(|ty| ty.receiver_owner().cloned())
			.filter(java_external_target_shape)
			.or_else(|| {
				node.child_by_field_name("object")
					.and_then(|object| expression_external_owner(state, object, owner, env))
			}),
		_ => None,
	}
}

fn infer_value_type(
	state: &JavaDiscover<'_>,
	value: Node<'_>,
	owner: &Moniker,
	env: &TypeEnv,
) -> Option<TypeExpr> {
	match value.kind() {
		"object_creation_expression" => value
			.child_by_field_name("type")
			.and_then(|ty| type_expr(state, ty)),
		"cast_expression" => value
			.child_by_field_name("type")
			.and_then(|ty| type_expr(state, ty)),
		"method_invocation" => infer_call_type(state, value, owner, env),
		"identifier" => env.resolve_local(node_slice(value, state.source)).cloned(),
		_ => None,
	}
}

fn infer_call_type(
	state: &JavaDiscover<'_>,
	call: Node<'_>,
	owner: &Moniker,
	env: &TypeEnv,
) -> Option<TypeExpr> {
	let name = call.child_by_field_name("name")?;
	let name = node_slice(name, state.source);
	let arity = call
		.child_by_field_name("arguments")
		.map(argument_count)
		.unwrap_or(0);
	let receiver = call
		.child_by_field_name("object")
		.and_then(|object| receiver_owner(state, object, owner, env))
		.or_else(|| enclosing_type(owner))?;
	state
		.return_types
		.get(&(receiver.clone(), name.to_vec(), arity))
		.cloned()
		.or_else(|| {
			(java_external_target_shape(&receiver)).then(|| {
				TypeExpr::external_opaque(extend_arity_call(&receiver, kinds::METHOD, name, arity))
			})
		})
}

fn method_target(state: &JavaDiscover<'_>, owner: &Moniker, name: &[u8], arity: usize) -> Moniker {
	lookup_callable(state, owner, name, arity)
		.unwrap_or_else(|| extend_arity_call(owner, kinds::METHOD, name, arity))
}

fn lookup_callable(
	state: &JavaDiscover<'_>,
	owner: &Moniker,
	name: &[u8],
	arity: usize,
) -> Option<Moniker> {
	state
		.callables
		.get(&(owner.clone(), name.to_vec(), arity))
		.map(|segment| extend_segment(owner, kinds::METHOD, segment))
}

fn owner_confidence(state: &JavaDiscover<'_>, owner: &Moniker) -> &'static [u8] {
	if java_external_target_shape(owner) {
		kinds::CONF_EXTERNAL
	} else if state.type_table.values().any(|target| target == owner) {
		kinds::CONF_RESOLVED
	} else {
		kinds::CONF_IMPORTED
	}
}

fn lookup_known_type_name(
	state: &JavaDiscover<'_>,
	name: &[u8],
) -> Option<(Moniker, &'static [u8])> {
	if let Some(target) = state.type_table.get(name) {
		return Some((target.clone(), kinds::CONF_RESOLVED));
	}
	if let Some(import) = state.imports.iter().find(|import| import.name == name) {
		return Some((import.target.clone(), import.confidence));
	}
	if builtins::is_java_lang_type(name) {
		return Some((java_lang_target(&state.root, name), kinds::CONF_EXTERNAL));
	}
	None
}

fn resolve_type_target(
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

pub(super) fn type_expr(state: &JavaDiscover<'_>, node: Node<'_>) -> Option<TypeExpr> {
	let name = type_name(node, state.source)?;
	if builtins::is_primitive_type(&name) || builtins::is_inferred_local_type(&name) {
		return None;
	}
	Some(TypeExpr::resolved(
		resolve_type_target(state, &name, kinds::CLASS).0,
	))
}

fn emit_type_refs(state: &mut JavaDiscover<'_>, node: Node<'_>, source: &Moniker) {
	match node.kind() {
		"type_identifier" | "scoped_type_identifier" | "generic_type" => {
			if let Some(name) = type_name(node, state.source) {
				if name.is_empty()
					|| builtins::is_primitive_type(&name)
					|| builtins::is_inferred_local_type(&name)
				{
					return;
				}
				let (target, confidence) = resolve_type_target(state, &name, kinds::CLASS);
				state.push_ref(ResolvedRef {
					source: source.clone(),
					target,
					kind: kinds::USES_TYPE,
					position: Some(node_position(node)),
					confidence,
					hints: RefHints::default(),
				});
			}
			if node.kind() == "generic_type"
				&& let Some(args) = node.child_by_field_name("type_arguments")
			{
				for child in named_children(args) {
					emit_type_refs(state, child, source);
				}
			}
		}
		"array_type" => {
			if let Some(element) = node.child_by_field_name("element") {
				emit_type_refs(state, element, source);
			}
		}
		_ => {
			for child in named_children(node) {
				emit_type_refs(state, child, source);
			}
		}
	}
}

fn annotations(state: &mut JavaDiscover<'_>, node: Node<'_>, source: &Moniker) {
	for child in named_children(node).filter(|child| child.kind() == "modifiers") {
		for annotation in named_children(child) {
			if matches!(annotation.kind(), "marker_annotation" | "annotation") {
				annotation_ref(state, annotation, source);
			}
		}
	}
}

fn param_annotations(state: &mut JavaDiscover<'_>, param: Node<'_>, callable: &Moniker) {
	let Some(name_node) = param.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	if name.is_empty() {
		return;
	}
	let source = extend_segment(callable, kinds::PARAM, name);
	annotations(state, param, &source);
}

fn annotation_ref(state: &mut JavaDiscover<'_>, node: Node<'_>, source: &Moniker) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let name = last_identifier(name_node, state.source);
	if name.is_empty() {
		return;
	}
	let (target, confidence) = resolve_type_target(state, &name, kinds::ANNOTATION_TYPE);
	state.push_ref(ResolvedRef {
		source: source.clone(),
		target,
		kind: kinds::ANNOTATES,
		position: Some(node_position(node)),
		confidence,
		hints: RefHints::default(),
	});
}

fn heritage_refs(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	source: &Moniker,
	kind: &'static [u8],
) {
	for child in named_children(node) {
		if matches!(
			child.kind(),
			"type_identifier" | "scoped_type_identifier" | "generic_type"
		) {
			let Some(name) = type_name(child, state.source) else {
				continue;
			};
			let target_kind = if kind == kinds::IMPLEMENTS {
				kinds::INTERFACE
			} else {
				kinds::CLASS
			};
			let (target, confidence) = resolve_type_target(state, &name, target_kind);
			state.push_ref(ResolvedRef {
				source: source.clone(),
				target,
				kind,
				position: Some(node_position(child)),
				confidence,
				hints: RefHints::default(),
			});
		} else {
			heritage_refs(state, child, source, kind);
		}
	}
}

fn argument_count(args: Node<'_>) -> usize {
	named_children(args).count()
}

fn extend_arity_call(parent: &Moniker, kind: &[u8], name: &[u8], arity: usize) -> Moniker {
	let slots = vec![CallableSlot::default(); arity];
	extend_callable_slots(parent, kind, name, &slots)
}

fn enclosing_type(scope: &Moniker) -> Option<Moniker> {
	let view = scope.as_view();
	let segments = view.segments().collect::<Vec<_>>();
	let index = segments.iter().rposition(|segment| {
		matches!(
			segment.kind,
			kinds::CLASS | kinds::INTERFACE | kinds::ENUM | kinds::RECORD | kinds::ANNOTATION_TYPE
		)
	})?;
	let mut builder = MonikerBuilder::new();
	builder.project(view.project());
	for segment in &segments[..=index] {
		builder.segment(segment.kind, segment.name);
	}
	Some(builder.build())
}

fn receiver_hint_bytes<'src>(receiver: Node<'src>, source: &'src [u8]) -> &'src [u8] {
	match receiver.kind() {
		"this" => crate::lang::kinds::HINT_THIS,
		"super" => crate::lang::kinds::HINT_SUPER,
		"identifier" => node_slice(receiver, source),
		"method_invocation" => crate::lang::kinds::HINT_CALL,
		"field_access" | "scoped_identifier" => crate::lang::kinds::HINT_MEMBER,
		_ => b"",
	}
}
