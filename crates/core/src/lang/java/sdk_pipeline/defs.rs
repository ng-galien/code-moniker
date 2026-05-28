use tree_sitter::Node;

use crate::core::code_graph::Position;
use crate::core::moniker::Moniker;
use crate::lang::callable::{
	CallableSlot, callable_segment_slots, extend_callable_slots, extend_segment,
	join_bytes_with_comma, normalize_type_text,
};
use crate::lang::sdk::{DiscoveredDef, Namespace, RefHints, ResolvedRef, TypeExpr};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::discover::JavaDiscover;
use super::syntax::{named_children, type_parameters};
use super::type_resolution::type_expr;

pub(super) fn collect_defs(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	for child in named_children(node) {
		match child.kind() {
			"class_declaration" => type_def(state, child, scope, kinds::CLASS),
			"interface_declaration" => type_def(state, child, scope, kinds::INTERFACE),
			"enum_declaration" => type_def(state, child, scope, kinds::ENUM),
			"record_declaration" => record_def(state, child, scope),
			"annotation_type_declaration" => type_def(state, child, scope, kinds::ANNOTATION_TYPE),
			"method_declaration" => callable_def(state, child, scope, kinds::METHOD),
			"constructor_declaration" => callable_def(state, child, scope, kinds::CONSTRUCTOR),
			"field_declaration" => field_defs(state, child, scope),
			_ => collect_defs(state, child, scope),
		}
	}
}

fn type_def(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker, kind: &'static [u8]) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	let moniker = extend_segment(scope, kind, name);
	state
		.type_table
		.entry(name.to_vec())
		.or_insert(moniker.clone());
	register_type_parameters(state, node, &moniker);
	state.push_def(def_record(DefInput {
		moniker: moniker.clone(),
		parent: scope.clone(),
		namespace: Namespace::Type,
		name: name.to_vec(),
		kind,
		visibility: visibility_of(node),
		signature: Vec::new(),
		position: Some(node_position(node)),
		call_name: Vec::new(),
		call_arity: None,
	}));
	if kind == kinds::ENUM {
		enum_constants(state, node, &moniker);
	}
	if let Some(body) = node.child_by_field_name("body") {
		collect_defs(state, body, &moniker);
	}
}

fn record_def(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	type_def(state, node, scope, kinds::RECORD);
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let record = extend_segment(scope, kinds::RECORD, node_slice(name_node, state.source));
	let explicit_accessors = explicit_no_arg_methods(node, state.source);
	let Some(params) = node.child_by_field_name("parameters") else {
		return;
	};
	for component in named_children(params) {
		record_component_def(state, &record, component, &explicit_accessors);
	}
}

fn record_component_def(
	state: &mut JavaDiscover<'_>,
	record: &Moniker,
	component: Node<'_>,
	explicit_accessors: &[Vec<u8>],
) {
	if !matches!(component.kind(), "formal_parameter" | "spread_parameter") {
		return;
	}
	let Some(name_node) = component.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	if name.is_empty() {
		return;
	}
	let ty = component
		.child_by_field_name("type")
		.and_then(|node| type_expr(state, node, record));
	if let Some(ty) = ty.clone() {
		state
			.field_types
			.insert((record.clone(), name.to_vec()), ty.clone());
		state
			.return_types
			.insert((record.clone(), name.to_vec(), 0), ty);
	}
	state.push_def(def_record(DefInput {
		moniker: extend_segment(record, kinds::FIELD, name),
		parent: record.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::FIELD,
		visibility: kinds::VIS_PRIVATE,
		signature: Vec::new(),
		position: Some(node_position(component)),
		call_name: Vec::new(),
		call_arity: None,
	}));
	if !explicit_accessors.iter().any(|accessor| accessor == name) {
		push_record_accessor(state, record, name, component);
		if let Some(ty) = ty {
			push_returns_type_ref(
				state,
				&extend_callable_slots(record, kinds::METHOD, name, &[]),
				ty,
				component,
			);
		}
	}
}

fn push_record_accessor(
	state: &mut JavaDiscover<'_>,
	record: &Moniker,
	name: &[u8],
	node: Node<'_>,
) {
	state.callables.insert(
		(record.clone(), name.to_vec(), 0),
		callable_segment_slots(name, &[]),
	);
	state.push_def(def_record(DefInput {
		moniker: extend_callable_slots(record, kinds::METHOD, name, &[]),
		parent: record.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::METHOD,
		visibility: kinds::VIS_PUBLIC,
		signature: Vec::new(),
		position: Some(node_position(node)),
		call_name: name.to_vec(),
		call_arity: Some(0),
	}));
}

fn enum_constants(state: &mut JavaDiscover<'_>, node: Node<'_>, enum_moniker: &Moniker) {
	let Some(body) = node.child_by_field_name("body") else {
		return;
	};
	for child in named_children(body).filter(|child| child.kind() == "enum_constant") {
		let Some(name_node) = child.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, state.source);
		state.push_def(def_record(DefInput {
			moniker: extend_segment(enum_moniker, kinds::ENUM_CONSTANT, name),
			parent: enum_moniker.clone(),
			namespace: Namespace::Value,
			name: name.to_vec(),
			kind: kinds::ENUM_CONSTANT,
			visibility: kinds::VIS_PUBLIC,
			signature: Vec::new(),
			position: Some(node_position(child)),
			call_name: Vec::new(),
			call_arity: None,
		}));
	}
}

fn callable_def(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	kind: &'static [u8],
) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	let slots = formal_parameter_slots(node, state.source);
	let signature =
		join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
	let moniker = extend_callable_slots(scope, kind, name, &slots);
	register_type_parameters(state, node, &moniker);
	state.callables.insert(
		(scope.clone(), name.to_vec(), slots.len()),
		callable_segment_slots(name, &slots),
	);
	if let Some(ty) = node
		.child_by_field_name("type")
		.and_then(|node| type_expr(state, node, &moniker))
	{
		state
			.return_types
			.insert((scope.clone(), name.to_vec(), slots.len()), ty);
	}
	state.push_def(def_record(DefInput {
		moniker: moniker.clone(),
		parent: scope.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind,
		visibility: visibility_of(node),
		signature,
		position: Some(node_position(node)),
		call_name: name.to_vec(),
		call_arity: Some(slots.len()),
	}));
	if let Some(ty) = node
		.child_by_field_name("type")
		.and_then(|node| type_expr(state, node, &moniker))
	{
		push_returns_type_ref(state, &moniker, ty, node);
	}
	if state.deep
		&& let Some(params) = node.child_by_field_name("parameters")
	{
		param_defs(state, params, &moniker);
	}
	if let Some(body) = node.child_by_field_name("body") {
		local_defs(state, body, &moniker);
	}
}

fn push_returns_type_ref(
	state: &mut JavaDiscover<'_>,
	source: &Moniker,
	ty: TypeExpr,
	node: Node<'_>,
) {
	let Some(target) = ty.receiver_owner().cloned() else {
		return;
	};
	state.push_ref(ResolvedRef {
		source: source.clone(),
		target,
		kind: kinds::RETURNS_TYPE,
		position: Some(node_position(node)),
		confidence: kinds::CONF_RESOLVED,
		hints: RefHints::default(),
	});
}

fn param_defs(state: &mut JavaDiscover<'_>, params: Node<'_>, callable: &Moniker) {
	for param in named_children(params) {
		if !matches!(param.kind(), "formal_parameter" | "spread_parameter") {
			continue;
		}
		let Some(name_node) = param.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, state.source);
		if !name.is_empty() {
			state.push_def(local_def(
				callable,
				kinds::PARAM,
				name.to_vec(),
				Some(node_position(param)),
			));
		}
	}
}

fn local_defs(state: &mut JavaDiscover<'_>, node: Node<'_>, callable: &Moniker) {
	if node.kind() == "local_variable_declaration" {
		for declarator in named_children(node).filter(|child| child.kind() == "variable_declarator")
		{
			let Some(name_node) = declarator.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, state.source);
			if !name.is_empty() {
				state.push_def(local_def(
					callable,
					kinds::LOCAL,
					name.to_vec(),
					Some(node_position(declarator)),
				));
			}
		}
	}
	for child in named_children(node) {
		local_defs(state, child, callable);
	}
}

fn field_defs(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let ty = node
		.child_by_field_name("type")
		.and_then(|node| type_expr(state, node, scope));
	for declarator in named_children(node).filter(|child| child.kind() == "variable_declarator") {
		let Some(name_node) = declarator.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, state.source);
		if name.is_empty() {
			continue;
		}
		if let Some(ty) = ty.clone() {
			state.field_types.insert((scope.clone(), name.to_vec()), ty);
		}
		state.push_def(def_record(DefInput {
			moniker: extend_segment(scope, kinds::FIELD, name),
			parent: scope.clone(),
			namespace: Namespace::Value,
			name: name.to_vec(),
			kind: kinds::FIELD,
			visibility: visibility_of(node),
			signature: Vec::new(),
			position: Some(node_position(declarator)),
			call_name: Vec::new(),
			call_arity: None,
		}));
	}
}

struct DefInput {
	moniker: Moniker,
	parent: Moniker,
	namespace: Namespace,
	name: Vec<u8>,
	kind: &'static [u8],
	visibility: &'static [u8],
	signature: Vec<u8>,
	position: Option<Position>,
	call_name: Vec<u8>,
	call_arity: Option<usize>,
}

fn def_record(input: DefInput) -> DiscoveredDef {
	DiscoveredDef {
		moniker: input.moniker,
		parent: input.parent,
		namespace: input.namespace,
		name: input.name,
		kind: input.kind,
		visibility: input.visibility,
		signature: input.signature,
		position: input.position,
		call_name: input.call_name,
		call_arity: input.call_arity,
	}
}

fn register_type_parameters(state: &mut JavaDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let params = type_parameters(node, state.source);
	if !params.is_empty() {
		state.type_params.insert(scope.clone(), params);
	}
}

fn local_def(
	function: &Moniker,
	kind: &'static [u8],
	name: Vec<u8>,
	position: Option<Position>,
) -> DiscoveredDef {
	def_record(DefInput {
		moniker: extend_segment(function, kind, &name),
		parent: function.clone(),
		namespace: Namespace::Value,
		name,
		kind,
		visibility: kinds::VIS_PACKAGE,
		signature: Vec::new(),
		position,
		call_name: Vec::new(),
		call_arity: None,
	})
}

pub(super) fn formal_parameter_slots(callable: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let Some(params) = callable.child_by_field_name("parameters") else {
		return Vec::new();
	};
	named_children(params)
		.filter(|child| matches!(child.kind(), "formal_parameter" | "spread_parameter"))
		.map(|child| {
			let r#type = child
				.child_by_field_name("type")
				.and_then(|ty| ty.utf8_text(source).ok())
				.map(normalize_type_text)
				.unwrap_or_default();
			let name = child
				.child_by_field_name("name")
				.map(|name| node_slice(name, source).to_vec())
				.unwrap_or_default();
			CallableSlot { name, r#type }
		})
		.collect()
}

fn slot_signature_bytes(slot: &CallableSlot) -> Vec<u8> {
	match (slot.name.as_slice(), slot.r#type.as_slice()) {
		(b"", b"") => b"_".to_vec(),
		(name, b"") => name.to_vec(),
		(b"", ty) => ty.to_vec(),
		(name, ty) => {
			let mut out = Vec::with_capacity(name.len() + 1 + ty.len());
			out.extend_from_slice(name);
			out.push(b':');
			out.extend_from_slice(ty);
			out
		}
	}
}

fn explicit_no_arg_methods(record_node: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	let Some(body) = record_node.child_by_field_name("body") else {
		return Vec::new();
	};
	named_children(body)
		.filter(|child| child.kind() == "method_declaration")
		.filter(|child| formal_parameter_slots(*child, source).is_empty())
		.filter_map(|child| child.child_by_field_name("name"))
		.map(|name| node_slice(name, source).to_vec())
		.collect()
}

fn visibility_of(node: Node<'_>) -> &'static [u8] {
	for child in named_children(node).filter(|child| child.kind() == "modifiers") {
		for modifier in named_children(child) {
			match modifier.kind() {
				"public" => return kinds::VIS_PUBLIC,
				"protected" => return kinds::VIS_PROTECTED,
				"private" => return kinds::VIS_PRIVATE,
				_ => {}
			}
		}
	}
	kinds::VIS_PACKAGE
}
