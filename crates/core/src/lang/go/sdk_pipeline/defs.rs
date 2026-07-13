use tree_sitter::Node;

use crate::core::code_graph::Position;
use crate::core::moniker::Moniker;
use crate::lang::callable::{
	CallableSlot, callable_segment_slots, extend_callable_slots, extend_segment,
	extend_segment_u32, join_bytes_with_comma, normalize_type_text, slot_signature_bytes,
};
use crate::lang::sdk::{DiscoveredDef, Namespace, RefHints, ResolvedRef, TypeExpr};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::discover::GoDiscover;
use super::syntax::{
	function_param_slots, named_children, receiver_type_name, spec_children, struct_field_list,
	visibility_from_name,
};
use super::type_resolution::{result_type_expr, type_expr};

// First pass: register every file-local type name before methods and
// references look receivers up (a method may precede its type declaration).
pub(super) fn predeclare_types(state: &mut GoDiscover<'_>, root: Node<'_>, scope: &Moniker) {
	for decl in named_children(root) {
		if decl.kind() != "type_declaration" {
			continue;
		}
		for spec in named_children(decl) {
			let (kind, name_node) = match spec.kind() {
				"type_spec" => (type_spec_kind(spec), spec.child_by_field_name("name")),
				"type_alias" => (kinds::TYPE, spec.child_by_field_name("name")),
				_ => continue,
			};
			let Some(name_node) = name_node else { continue };
			let name = node_slice(name_node, state.source);
			if name.is_empty() {
				continue;
			}
			let moniker = extend_segment(scope, kind, name);
			state.push_def(DiscoveredDef {
				moniker: moniker.clone(),
				parent: scope.clone(),
				namespace: Namespace::Type,
				name: name.to_vec(),
				kind: kind_static(kind),
				visibility: visibility_from_name(name),
				signature: Vec::new(),
				position: Some(node_position(spec)),
				call_name: Vec::new(),
				call_arity: None,
			});
			state.type_table.entry(name.to_vec()).or_insert(moniker);
		}
	}
}

pub(super) fn type_spec_kind(spec: Node<'_>) -> &'static [u8] {
	match spec.child_by_field_name("type").map(|node| node.kind()) {
		Some("struct_type") => kinds::STRUCT,
		Some("interface_type") => kinds::INTERFACE,
		_ => kinds::TYPE,
	}
}

fn kind_static(kind: &[u8]) -> &'static [u8] {
	match kind {
		k if k == kinds::STRUCT => kinds::STRUCT,
		k if k == kinds::INTERFACE => kinds::INTERFACE,
		_ => kinds::TYPE,
	}
}

pub(super) fn collect_defs(state: &mut GoDiscover<'_>, root: Node<'_>, scope: &Moniker) {
	let mut cursor = root.walk();
	let mut pending_comment = None;
	for child in root.children(&mut cursor) {
		if child.kind() == "comment" {
			extend_or_flush_comment(state, &mut pending_comment, child, scope);
			continue;
		}
		flush_comment(state, &mut pending_comment, scope);
		if !child.is_named() {
			continue;
		}
		match child.kind() {
			"type_declaration" => type_declaration_defs(state, child, scope),
			"function_declaration" => function_def(state, child, scope),
			"method_declaration" => method_def(state, child, scope),
			"var_declaration" => module_value_defs(state, child, scope, "var_spec", kinds::VAR),
			"const_declaration" => {
				module_value_defs(state, child, scope, "const_spec", kinds::CONST)
			}
			_ => {}
		}
	}
	flush_comment(state, &mut pending_comment, scope);
}

// Comments between the specs of a grouped `type (...)` declaration surface as
// module-scope comment defs, matching the legacy walker.
fn type_declaration_defs(state: &mut GoDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let mut cursor = node.walk();
	let mut pending_comment = None;
	for child in node.children(&mut cursor) {
		if child.kind() == "comment" {
			extend_or_flush_comment(state, &mut pending_comment, child, scope);
			continue;
		}
		flush_comment(state, &mut pending_comment, scope);
		if child.is_named() {
			type_member_defs(state, child, scope);
		}
	}
	flush_comment(state, &mut pending_comment, scope);
}

fn type_member_defs(state: &mut GoDiscover<'_>, spec: Node<'_>, scope: &Moniker) {
	if spec.kind() != "type_spec" {
		return;
	}
	let Some(name_node) = spec.child_by_field_name("name") else {
		return;
	};
	let Some(type_node) = spec.child_by_field_name("type") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	let owner = extend_segment(scope, type_spec_kind(spec), name);
	match type_node.kind() {
		"struct_type" => struct_member_defs(state, type_node, &owner),
		"interface_type" => interface_member_defs(state, type_node, &owner),
		_ => {}
	}
}

fn struct_member_defs(state: &mut GoDiscover<'_>, struct_node: Node<'_>, owner: &Moniker) {
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
		let ty = type_expr(state, type_node);
		if field.child_by_field_name("name").is_none() {
			embedded_field_type(state, type_node, owner, ty);
			continue;
		}
		let signature = normalize_type_text(type_node.utf8_text(state.source).unwrap_or_default());
		for name_node in named_children(field).filter(|node| node.kind() == "field_identifier") {
			named_field_def(state, owner, name_node, ty.as_ref(), &signature);
		}
	}
}

fn named_field_def(
	state: &mut GoDiscover<'_>,
	owner: &Moniker,
	name_node: Node<'_>,
	ty: Option<&TypeExpr>,
	signature: &[u8],
) {
	let name = node_slice(name_node, state.source);
	if name.is_empty() {
		return;
	}
	let field_moniker = extend_segment(owner, kinds::FIELD, name);
	if let Some(ty) = ty {
		push_typed_as_ref(state, &field_moniker, ty, name_node);
		state
			.field_types
			.insert((owner.clone(), name.to_vec()), ty.clone());
	}
	state.push_def(DiscoveredDef {
		moniker: field_moniker,
		parent: owner.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::FIELD,
		visibility: visibility_from_name(name),
		signature: signature.to_vec(),
		position: Some(node_position(name_node)),
		call_name: Vec::new(),
		call_arity: None,
	});
}

// An embedded field is addressable by the terminal name of its type
// (`p.Sampler` on `struct { Sampler }`); record it so receiver typing sees it.
fn embedded_field_type(
	state: &mut GoDiscover<'_>,
	type_node: Node<'_>,
	owner: &Moniker,
	ty: Option<TypeExpr>,
) {
	let Some(ty) = ty else { return };
	let Some(target) = ty.receiver_owner() else {
		return;
	};
	let Some(last) = target.as_view().segments().last() else {
		return;
	};
	let _ = type_node;
	state
		.field_types
		.entry((owner.clone(), last.name.to_vec()))
		.or_insert(ty.clone());
}

fn push_typed_as_ref(state: &mut GoDiscover<'_>, field: &Moniker, ty: &TypeExpr, node: Node<'_>) {
	let Some(target) = ty.receiver_owner().cloned() else {
		return;
	};
	state.push_ref(ResolvedRef {
		source: field.clone(),
		target,
		kind: kinds::TYPED_AS,
		position: Some(node_position(node)),
		confidence: kinds::CONF_RESOLVED,
		hints: RefHints::default(),
	});
}

fn interface_member_defs(state: &mut GoDiscover<'_>, interface_node: Node<'_>, owner: &Moniker) {
	for child in named_children(interface_node) {
		if child.kind() != "method_elem" {
			continue;
		}
		let Some(name_node) = child.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, state.source);
		if name.is_empty() {
			continue;
		}
		let slots = function_param_slots(child, state.source);
		let moniker = extend_callable_slots(owner, kinds::METHOD, name, &slots);
		state.callables.insert(
			(owner.clone(), name.to_vec()),
			callable_segment_slots(name, &slots),
		);
		if let Some(ty) = child
			.child_by_field_name("result")
			.and_then(|result| result_type_expr(state, result))
		{
			state
				.return_types
				.insert((owner.clone(), name.to_vec()), ty);
		}
		state.push_def(DiscoveredDef {
			moniker,
			parent: owner.clone(),
			namespace: Namespace::Value,
			name: name.to_vec(),
			kind: kinds::METHOD,
			visibility: visibility_from_name(name),
			signature: callable_signature(&slots),
			position: Some(node_position(child)),
			call_name: name.to_vec(),
			call_arity: Some(slots.len()),
		});
	}
}

fn function_def(state: &mut GoDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	let slots = function_param_slots(node, state.source);
	let moniker = extend_callable_slots(scope, kinds::FUNC, name, &slots);
	callable_tables(state, node, scope, name, &slots);
	state.push_def(DiscoveredDef {
		moniker: moniker.clone(),
		parent: scope.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::FUNC,
		visibility: visibility_from_name(name),
		signature: callable_signature(&slots),
		position: Some(node_position(node)),
		call_name: name.to_vec(),
		call_arity: Some(slots.len()),
	});
	callable_scope_defs(state, node, &moniker);
}

// A receiver type declared in a sibling file still nests the method moniker
// under the same-package fallback owner, but only a file-local type def can
// serve as the graph parent; otherwise the module root does.
fn method_def(state: &mut GoDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let Some(receiver) = node.child_by_field_name("receiver") else {
		return;
	};
	let Some(receiver_name) = receiver_type_name(receiver, state.source) else {
		return;
	};
	let local_owner = state.type_table.get(receiver_name).cloned();
	let owner = local_owner
		.clone()
		.unwrap_or_else(|| extend_segment(scope, kinds::STRUCT, receiver_name));
	let name = node_slice(name_node, state.source);
	let slots = function_param_slots(node, state.source);
	let moniker = extend_callable_slots(&owner, kinds::METHOD, name, &slots);
	callable_tables(state, node, &owner, name, &slots);
	state.push_def(DiscoveredDef {
		moniker: moniker.clone(),
		parent: local_owner.unwrap_or_else(|| scope.clone()),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::METHOD,
		visibility: visibility_from_name(name),
		signature: callable_signature(&slots),
		position: Some(node_position(node)),
		call_name: name.to_vec(),
		call_arity: Some(slots.len()),
	});
	callable_scope_defs(state, node, &moniker);
}

fn callable_tables(
	state: &mut GoDiscover<'_>,
	node: Node<'_>,
	owner: &Moniker,
	name: &[u8],
	slots: &[CallableSlot],
) {
	state.callables.insert(
		(owner.clone(), name.to_vec()),
		callable_segment_slots(name, slots),
	);
	if let Some(ty) = node
		.child_by_field_name("result")
		.and_then(|result| result_type_expr(state, result))
	{
		state
			.return_types
			.insert((owner.clone(), name.to_vec()), ty);
	}
}

fn callable_signature(slots: &[CallableSlot]) -> Vec<u8> {
	join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>())
}

// Inside a callable: parameter and local defs in deep mode, merged comment
// defs in every mode (the walk covers nested blocks).
fn callable_scope_defs(state: &mut GoDiscover<'_>, node: Node<'_>, callable: &Moniker) {
	if state.deep {
		if let Some(receiver) = node.child_by_field_name("receiver") {
			param_defs(state, receiver, callable);
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			param_defs(state, params, callable);
		}
	}
	if let Some(body) = node.child_by_field_name("body") {
		body_defs(state, body, callable);
	}
}

fn param_defs(state: &mut GoDiscover<'_>, container: Node<'_>, callable: &Moniker) {
	for child in named_children(container) {
		if !matches!(
			child.kind(),
			"parameter_declaration" | "variadic_parameter_declaration"
		) {
			continue;
		}
		for name_node in named_children(child).filter(|node| node.kind() == "identifier") {
			let name = node_slice(name_node, state.source);
			if !name.is_empty() && name != b"_" {
				push_binding_def(
					state,
					callable,
					kinds::PARAM,
					name,
					node_position(name_node),
				);
			}
		}
	}
}

fn body_defs(state: &mut GoDiscover<'_>, node: Node<'_>, callable: &Moniker) {
	if state.deep {
		match node.kind() {
			"short_var_declaration" => {
				if let Some(left) = node.child_by_field_name("left") {
					binding_defs_in_expression_list(state, left, callable);
				}
			}
			"range_clause" => {
				if let Some(left) = node.child_by_field_name("left") {
					binding_defs_in_expression_list(state, left, callable);
				}
			}
			"var_declaration" => local_spec_defs(state, node, callable, "var_spec"),
			"const_declaration" => local_spec_defs(state, node, callable, "const_spec"),
			_ => {}
		}
	}
	let mut cursor = node.walk();
	let mut pending_comment = None;
	for child in node.children(&mut cursor) {
		if child.kind() == "comment" {
			extend_or_flush_comment(state, &mut pending_comment, child, callable);
			continue;
		}
		flush_comment(state, &mut pending_comment, callable);
		if child.is_named() {
			body_defs(state, child, callable);
		}
	}
	flush_comment(state, &mut pending_comment, callable);
}

fn local_spec_defs(
	state: &mut GoDiscover<'_>,
	node: Node<'_>,
	callable: &Moniker,
	spec_kind: &str,
) {
	for spec in spec_children(node, spec_kind) {
		for name_node in named_children(spec).filter(|node| node.kind() == "identifier") {
			let name = node_slice(name_node, state.source);
			if !name.is_empty() && name != b"_" {
				push_binding_def(
					state,
					callable,
					kinds::LOCAL,
					name,
					node_position(name_node),
				);
			}
		}
	}
}

fn binding_defs_in_expression_list(state: &mut GoDiscover<'_>, node: Node<'_>, callable: &Moniker) {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, state.source);
			if !name.is_empty() && name != b"_" {
				push_binding_def(state, callable, kinds::LOCAL, name, node_position(node));
			}
		}
		"expression_list" => {
			for child in named_children(node) {
				binding_defs_in_expression_list(state, child, callable);
			}
		}
		_ => {}
	}
}

fn push_binding_def(
	state: &mut GoDiscover<'_>,
	callable: &Moniker,
	kind: &'static [u8],
	name: &[u8],
	position: Position,
) {
	state.push_def(DiscoveredDef {
		moniker: extend_segment(callable, kind, name),
		parent: callable.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind,
		visibility: kinds::VIS_NONE,
		signature: Vec::new(),
		position: Some(position),
		call_name: Vec::new(),
		call_arity: None,
	});
}

fn module_value_defs(
	state: &mut GoDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	spec_kind: &str,
	def_kind: &'static [u8],
) {
	for spec in spec_children(node, spec_kind) {
		for name_node in named_children(spec).filter(|node| node.kind() == "identifier") {
			let name = node_slice(name_node, state.source);
			if name.is_empty() || name == b"_" {
				continue;
			}
			state.push_def(DiscoveredDef {
				moniker: extend_segment(scope, def_kind, name),
				parent: scope.clone(),
				namespace: Namespace::Value,
				name: name.to_vec(),
				kind: def_kind,
				visibility: visibility_from_name(name),
				signature: Vec::new(),
				position: Some(node_position(name_node)),
				call_name: Vec::new(),
				call_arity: None,
			});
		}
	}
}

struct PendingComment {
	start_byte: u32,
	end_byte: u32,
	end_row: usize,
}

fn extend_or_flush_comment(
	state: &mut GoDiscover<'_>,
	pending: &mut Option<PendingComment>,
	node: Node<'_>,
	scope: &Moniker,
) {
	let start_row = node.start_position().row;
	let end_row = node.end_position().row;
	let start_byte = node.start_byte() as u32;
	let end_byte = node.end_byte() as u32;
	if let Some(comment) = pending.as_mut() {
		if start_row <= comment.end_row + 1 {
			comment.end_byte = end_byte;
			comment.end_row = end_row;
			return;
		}
		state.push_def(comment_def(scope, comment.start_byte, comment.end_byte));
	}
	*pending = Some(PendingComment {
		start_byte,
		end_byte,
		end_row,
	});
}

fn flush_comment(
	state: &mut GoDiscover<'_>,
	pending: &mut Option<PendingComment>,
	scope: &Moniker,
) {
	if let Some(comment) = pending.take() {
		state.push_def(comment_def(scope, comment.start_byte, comment.end_byte));
	}
}

fn comment_def(scope: &Moniker, start_byte: u32, end_byte: u32) -> DiscoveredDef {
	DiscoveredDef {
		moniker: extend_segment_u32(scope, kinds::COMMENT, start_byte),
		parent: scope.clone(),
		namespace: Namespace::Custom("annotation"),
		name: start_byte.to_string().into_bytes(),
		kind: kinds::COMMENT,
		visibility: kinds::VIS_NONE,
		signature: Vec::new(),
		position: Some((start_byte, end_byte)),
		call_name: Vec::new(),
		call_arity: None,
	}
}
