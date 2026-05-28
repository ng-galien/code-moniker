use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::callable::{CallableSlot, extend_callable_slots, extend_segment};
use crate::lang::sdk::{RefHints, ResolvedRef, TypeEnv, TypeExpr};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::builtins;
use super::defs::formal_parameter_slots;
use super::discover::JavaDiscover;
use super::imports::{java_external_target_shape, java_lang_target};
use super::syntax::{last_identifier, named_children, type_name, type_parameters, type_path};
use super::type_resolution::{
	is_type_param_in_scope, lookup_known_type_name, resolve_type_path, resolve_type_target,
	same_package_type_target, type_env_for_scope, type_expr,
};

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
	register_callable_type_parameters(state, node, &callable);
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
		let type_env = callable_type_env(state, node, body, &callable);
		expr_refs(state, body, &callable, scope, &type_env);
	}
}

fn register_callable_type_parameters(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	callable: &Moniker,
) {
	let params = type_parameters(node, state.source);
	if !params.is_empty() {
		state.type_params.entry(callable.clone()).or_insert(params);
	}
}

fn field_refs(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	if let Some(ty) = node.child_by_field_name("type") {
		emit_type_refs(state, ty, scope);
	}
	let type_env = type_env_for_scope(state, scope);
	for declarator in named_children(node).filter(|child| child.kind() == "variable_declarator") {
		if let Some(value) = declarator.child_by_field_name("value") {
			expr_refs(state, value, scope, scope, &type_env);
		}
	}
}

fn callable_type_env(
	state: &JavaDiscover<'_>,
	callable: Node<'_>,
	body: Node<'_>,
	owner: &Moniker,
) -> TypeEnv {
	let mut env = type_env_for_scope(state, owner);
	if let Some(params) = callable.child_by_field_name("parameters") {
		for param in named_children(params) {
			let Some(name_node) = param.child_by_field_name("name") else {
				continue;
			};
			let Some(ty) = param
				.child_by_field_name("type")
				.and_then(|node| type_expr(state, node, owner))
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
		let declared_type = type_node.and_then(|node| type_expr(state, node, owner));
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
		"method_declaration" | "constructor_declaration" => {
			return;
		}
		"object_creation_expression" => object_creation_ref(state, node, source),
		"local_variable_declaration" => {
			if let Some(ty) = node.child_by_field_name("type") {
				emit_type_refs(state, ty, source);
			}
		}
		"field_access" | "scoped_identifier" => {
			static_member_read_ref(state, node, source, owner);
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
	let (target, confidence, receiver_hint) = if let Some(object) =
		node.child_by_field_name("object")
	{
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
	} else if let Some((target, confidence)) = lookup_static_imported_callable(state, name, arity) {
		(target, confidence, Vec::new())
	} else {
		(
			extend_arity_call(owner, kinds::METHOD, name, arity),
			kinds::CONF_NAME_MATCH,
			Vec::new(),
		)
	};
	let hints = RefHints {
		receiver_hint,
		call_name: name.to_vec(),
		call_arity: Some(arity),
		..RefHints::default()
	};
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
	let Some(path) = type_path(ty, state.source) else {
		return;
	};
	let (target, confidence) = resolve_type_path(state, &path, kinds::CLASS);
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

fn static_member_read_ref(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	source: &Moniker,
	owner: &Moniker,
) {
	let Some(object) = node
		.child_by_field_name("object")
		.or_else(|| node.child_by_field_name("scope"))
	else {
		return;
	};
	let Some(member) = node
		.child_by_field_name("field")
		.or_else(|| node.child_by_field_name("name"))
	else {
		return;
	};
	if node_slice(member, state.source) == b"class" {
		return;
	}
	let Some(type_owner) = static_type_owner(state, object, owner) else {
		return;
	};
	if java_external_target_shape(&type_owner) {
		return;
	}
	state.push_ref(ResolvedRef {
		source: source.clone(),
		target: extend_segment(&type_owner, kinds::PATH, node_slice(member, state.source)),
		kind: kinds::READS,
		position: Some(node_position(node)),
		confidence: owner_confidence(state, &type_owner),
		hints: RefHints::default(),
	});
}

fn static_type_owner(state: &JavaDiscover<'_>, node: Node<'_>, owner: &Moniker) -> Option<Moniker> {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, state.source);
			name.first()
				.is_some_and(u8::is_ascii_uppercase)
				.then_some(())?;
			lookup_known_type_name(state, name)
				.map(|(target, _)| target)
				.or_else(|| same_package_type_target(state, name))
		}
		"type_identifier" | "scoped_type_identifier" | "generic_type" | "array_type" => {
			type_expr(state, node, owner).and_then(|ty| ty.receiver_owner().cloned())
		}
		_ => None,
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
				.or_else(|| same_package_type_target(state, name))
		}
		"object_creation_expression" => receiver
			.child_by_field_name("type")
			.and_then(|ty| type_expr(state, ty, owner))
			.and_then(|ty| ty.receiver_owner().cloned()),
		"field_access" | "scoped_identifier" => class_literal_owner(state, receiver)
			.or_else(|| expression_external_owner(state, receiver, owner, env)),
		"method_invocation" => {
			infer_call_type(state, receiver, owner, env).and_then(|ty| ty.receiver_owner().cloned())
		}
		"class_literal" => Some(java_lang_target(&state.root, b"Class")),
		"cast_expression" => cast_receiver_owner(state, receiver, owner),
		"parenthesized_expression" => parenthesized_receiver_owner(state, receiver, owner, env),
		"string_literal" => Some(java_lang_target(&state.root, b"String")),
		_ => None,
	}
}

fn cast_receiver_owner(
	state: &JavaDiscover<'_>,
	cast: Node<'_>,
	owner: &Moniker,
) -> Option<Moniker> {
	cast.child_by_field_name("type")
		.and_then(|ty| type_expr(state, ty, owner))
		.and_then(|ty| ty.receiver_owner().cloned())
}

fn parenthesized_receiver_owner(
	state: &JavaDiscover<'_>,
	node: Node<'_>,
	owner: &Moniker,
	env: &TypeEnv,
) -> Option<Moniker> {
	named_children(node)
		.next()
		.and_then(|expr| receiver_owner(state, expr, owner, env))
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
			let path = type_path(node, state.source)?;
			let (target, confidence) = resolve_type_path(state, &path, kinds::CLASS);
			(confidence == kinds::CONF_EXTERNAL).then_some(target)
		}
		"class_literal" => Some(java_lang_target(&state.root, b"Class")),
		"field_access" | "scoped_identifier" => node
			.child_by_field_name("field")
			.or_else(|| node.child_by_field_name("name"))
			.and_then(|field| (node_slice(field, state.source) == b"class").then_some(()))
			.map(|_| java_lang_target(&state.root, b"Class"))
			.or_else(|| {
				node.child_by_field_name("object")
					.and_then(|object| expression_external_owner(state, object, owner, env))
			}),
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

fn class_literal_owner(state: &JavaDiscover<'_>, node: Node<'_>) -> Option<Moniker> {
	node.child_by_field_name("field")
		.or_else(|| node.child_by_field_name("name"))
		.and_then(|field| (node_slice(field, state.source) == b"class").then_some(()))
		.map(|_| java_lang_target(&state.root, b"Class"))
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
			.and_then(|ty| type_expr(state, ty, owner)),
		"cast_expression" => value
			.child_by_field_name("type")
			.and_then(|ty| type_expr(state, ty, owner)),
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
	if call.child_by_field_name("object").is_none()
		&& let Some((target, _)) = lookup_static_imported_callable(state, name, arity)
	{
		return Some(TypeExpr::external_opaque(target));
	}
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

fn lookup_static_imported_callable(
	state: &JavaDiscover<'_>,
	name: &[u8],
	arity: usize,
) -> Option<(Moniker, &'static [u8])> {
	state
		.imports
		.iter()
		.find(|import| import.is_static && import.name == name)
		.map(|import| {
			(
				extend_arity_call(&import.target, kinds::METHOD, name, arity),
				import.confidence,
			)
		})
}

fn emit_type_refs(state: &mut JavaDiscover<'_>, node: Node<'_>, source: &Moniker) {
	match node.kind() {
		"type_identifier" | "scoped_type_identifier" | "generic_type" => {
			if let Some(path) = type_path(node, state.source) {
				let Some(name) = path.last() else {
					return;
				};
				if name.is_empty()
					|| builtins::is_primitive_type(name)
					|| builtins::is_inferred_local_type(name)
					|| (path.len() == 1 && is_type_param_in_scope(state, source, name))
				{
					return;
				}
				let (target, confidence) = resolve_type_path(state, &path, kinds::CLASS);
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
			let Some(path) = type_path(child, state.source) else {
				continue;
			};
			let target_kind = if kind == kinds::IMPLEMENTS {
				kinds::INTERFACE
			} else {
				kinds::CLASS
			};
			let (target, confidence) = resolve_type_path(state, &path, target_kind);
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
