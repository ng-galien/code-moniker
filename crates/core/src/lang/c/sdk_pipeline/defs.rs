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
use super::discover::CDiscover;
use super::syntax::{
	DeclaratorInfo, declarator_info, first_identifier, named_children, parameter_slots,
	recovered_identifier, visibility_for,
};
use super::type_resolution::type_expr_for;

pub(super) fn is_preproc_container(kind: &str) -> bool {
	matches!(
		kind,
		"preproc_ifdef"
			| "preproc_if"
			| "preproc_elif"
			| "preproc_else"
			| "linkage_specification"
			| "declaration_list"
	)
}

// First pass: register every file-local type name (named struct/union/enum
// specifiers and typedefs) before members, receivers and refs look them up.
pub(super) fn predeclare_types(state: &mut CDiscover<'_>, root: Node<'_>, scope: &Moniker) {
	predeclare_recovered_generated_aggregate(state, root, scope);
	for child in named_children(root) {
		match child.kind() {
			"type_definition" => predeclare_typedef(state, child, scope),
			"declaration" | "struct_specifier" | "union_specifier" | "enum_specifier" => {
				predeclare_specifier_of(state, child, scope);
			}
			kind if is_preproc_container(kind) => predeclare_types(state, child, scope),
			"ERROR" => predeclare_types(state, child, scope),
			_ => {}
		}
	}
}

fn predeclare_recovered_generated_aggregate(
	state: &mut CDiscover<'_>,
	root: Node<'_>,
	scope: &Moniker,
) {
	let Some((specifier, _body, alias)) = recovered_generated_aggregate(root, state.source) else {
		return;
	};
	let Some(name_node) = specifier.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	push_type_def(state, scope, kinds::STRUCT, name, node_position(specifier));
	push_type_def(
		state,
		scope,
		kinds::TYPE,
		node_slice(alias, state.source),
		node_position(alias),
	);
}

fn predeclare_typedef(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	predeclare_specifier_of(state, node, scope);
	for declarator in typedef_declarators(node) {
		let Some(name_node) =
			super::syntax::declarator_info(declarator, state.source).map(|info| info.name_node)
		else {
			continue;
		};
		let name = node_slice(name_node, state.source);
		push_type_def(state, scope, kinds::TYPE, name, node_position(node));
	}
}

fn typedef_declarators<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
	let mut cursor = node.walk();
	node.children_by_field_name("declarator", &mut cursor)
		.collect()
}

fn predeclare_specifier_of(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let specifier = if node.kind() == "declaration" || node.kind() == "type_definition" {
		node.child_by_field_name("type")
	} else {
		Some(node)
	};
	let Some(specifier) = specifier else { return };
	let kind = match specifier.kind() {
		"struct_specifier" | "union_specifier" => kinds::STRUCT,
		"enum_specifier" => kinds::ENUM,
		_ => return,
	};
	if specifier.child_by_field_name("body").is_none() {
		return;
	}
	let Some(name_node) = specifier.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	push_type_def(state, scope, kind, name, node_position(specifier));
}

fn push_type_def(
	state: &mut CDiscover<'_>,
	scope: &Moniker,
	kind: &'static [u8],
	name: &[u8],
	position: Position,
) {
	if name.is_empty() {
		return;
	}
	let moniker = extend_segment(scope, kind, name);
	state.push_def(DiscoveredDef {
		moniker: moniker.clone(),
		parent: scope.clone(),
		namespace: Namespace::Type,
		name: name.to_vec(),
		kind,
		visibility: kinds::VIS_PUBLIC,
		signature: Vec::new(),
		position: Some(position),
		call_name: Vec::new(),
		call_arity: None,
	});
	state.type_table.entry(name.to_vec()).or_insert(moniker);
}

pub(super) fn collect_defs(state: &mut CDiscover<'_>, root: Node<'_>, scope: &Moniker) {
	collect_recovered_generated_aggregate(state, root, scope);
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
			"function_definition" => function_def(state, child, scope),
			"declaration" => declaration_defs(state, child, scope),
			"type_definition" => typedef_defs(state, child, scope),
			"struct_specifier" | "union_specifier" | "enum_specifier" => {
				specifier_member_defs(state, child, scope, None);
			}
			"preproc_def" => object_macro_def(state, child, scope),
			"preproc_function_def" => function_macro_def(state, child, scope),
			kind if is_preproc_container(kind) => collect_defs(state, child, scope),
			"ERROR" => collect_defs(state, child, scope),
			_ => {}
		}
	}
	flush_comment(state, &mut pending_comment, scope);
}

fn collect_recovered_generated_aggregate(
	state: &mut CDiscover<'_>,
	root: Node<'_>,
	scope: &Moniker,
) {
	let Some((specifier, body, _alias)) = recovered_generated_aggregate(root, state.source) else {
		return;
	};
	let Some(name_node) = specifier.child_by_field_name("name") else {
		return;
	};
	let owner = extend_segment(scope, kinds::STRUCT, node_slice(name_node, state.source));
	for declaration in named_children(body) {
		if declaration.kind() != "declaration" {
			continue;
		}
		let type_node = declaration.child_by_field_name("type");
		for declarator in declaration_declarators(declaration) {
			let Some(info) = declarator_info(declarator, state.source) else {
				continue;
			};
			field_def(state, &owner, type_node, &info);
		}
	}
}

// Bison output commonly inserts `#line` directives between the aggregate
// name, body and trailing typedef alias. Tree-sitter then exposes those three
// pieces as siblings under the surrounding preprocessor conditional instead
// of one `type_definition`. Recover only that narrow generated-code shape.
fn recovered_generated_aggregate<'tree>(
	root: Node<'tree>,
	source: &[u8],
) -> Option<(Node<'tree>, Node<'tree>, Node<'tree>)> {
	if !matches!(root.kind(), "preproc_if" | "preproc_ifdef") {
		return None;
	}
	let children = named_children(root).collect::<Vec<_>>();
	for (specifier_index, error) in children.iter().enumerate() {
		if error.kind() != "ERROR" {
			continue;
		}
		let Some(specifier) = named_children(*error).find(|child| {
			matches!(child.kind(), "struct_specifier" | "union_specifier")
				&& child.child_by_field_name("name").is_some()
				&& child.child_by_field_name("body").is_none()
		}) else {
			continue;
		};
		let body_index = children
			.iter()
			.enumerate()
			.skip(specifier_index + 1)
			.find_map(|(index, child)| (child.kind() == "compound_statement").then_some(index))?;
		if !children[specifier_index + 1..body_index]
			.iter()
			.any(|child| is_line_directive(*child, source))
		{
			continue;
		}
		let Some((alias_index, alias)) =
			children
				.iter()
				.enumerate()
				.skip(body_index + 1)
				.find_map(|(index, child)| {
					(child.kind() == "expression_statement")
						.then(|| first_identifier(*child).map(|alias| (index, alias)))
						.flatten()
				})
		else {
			continue;
		};
		if !children[body_index + 1..alias_index]
			.iter()
			.any(|child| is_line_directive(*child, source))
		{
			continue;
		}
		let name = specifier.child_by_field_name("name")?;
		if node_slice(name, source) == node_slice(alias, source) {
			return Some((specifier, children[body_index], alias));
		}
	}
	None
}

fn is_line_directive(node: Node<'_>, source: &[u8]) -> bool {
	node.kind() == "preproc_call" && node_slice(node, source).starts_with(b"#line")
}

fn function_def(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
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
	let moniker = extend_callable_slots(scope, kinds::FUNC, info.name, &slots);
	register_callable(state, node, &info, &slots);
	state.upsert_def(DiscoveredDef {
		moniker: moniker.clone(),
		parent: scope.clone(),
		namespace: Namespace::Value,
		name: info.name.to_vec(),
		kind: kinds::FUNC,
		visibility: visibility_for(node, state.source),
		signature: callable_signature(&slots),
		position: Some(node_position(node)),
		call_name: info.name.to_vec(),
		call_arity: Some(slots.len()),
	});
	if state.deep {
		if let Some(params) = info.parameters {
			param_defs(state, params, &moniker);
		}
	}
	if let Some(body) = node.child_by_field_name("body") {
		body_defs(state, body, &moniker);
	}
}

fn register_callable(
	state: &mut CDiscover<'_>,
	node: Node<'_>,
	info: &DeclaratorInfo<'_>,
	slots: &[CallableSlot],
) {
	state
		.callables
		.insert(info.name.to_vec(), callable_segment_slots(info.name, slots));
	if let Some(ty) = node
		.child_by_field_name("type")
		.and_then(|type_node| type_expr_for(state, type_node, info.pointer_depth))
	{
		state.return_types.insert(info.name.to_vec(), ty);
	}
}

fn declaration_defs(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	specifier_member_defs_of(state, node, scope);
	let type_node = node.child_by_field_name("type");
	for declarator in declaration_declarators(node) {
		let Some(info) = declarator_info(declarator, state.source) else {
			continue;
		};
		if info.is_function {
			prototype_def(state, node, scope, &info);
			continue;
		}
		module_var_def(state, node, scope, type_node, &info);
	}
}

fn declaration_declarators<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
	let mut cursor = node.walk();
	node.children_by_field_name("declarator", &mut cursor)
		.map(|declarator| match declarator.kind() {
			"init_declarator" => declarator
				.child_by_field_name("declarator")
				.unwrap_or(declarator),
			_ => declarator,
		})
		.collect()
}

fn prototype_def(
	state: &mut CDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	info: &DeclaratorInfo<'_>,
) {
	let slots = info
		.parameters
		.map(|params| parameter_slots(params, state.source))
		.unwrap_or_default();
	register_callable(state, node, info, &slots);
	state.push_def(DiscoveredDef {
		moniker: extend_callable_slots(scope, kinds::FUNC, info.name, &slots),
		parent: scope.clone(),
		namespace: Namespace::Value,
		name: info.name.to_vec(),
		kind: kinds::FUNC,
		visibility: visibility_for(node, state.source),
		signature: callable_signature(&slots),
		position: Some(node_position(node)),
		call_name: info.name.to_vec(),
		call_arity: Some(slots.len()),
	});
}

fn module_var_def(
	state: &mut CDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	type_node: Option<Node<'_>>,
	info: &DeclaratorInfo<'_>,
) {
	let moniker = extend_segment(scope, kinds::VAR, info.name);
	let ty = type_node.and_then(|ty| type_expr_for(state, ty, info.pointer_depth));
	if let Some(ty) = &ty {
		push_typed_as_ref(state, &moniker, ty, info.name_node);
		state.var_types.insert(info.name.to_vec(), ty.clone());
	}
	state.push_def(DiscoveredDef {
		moniker,
		parent: scope.clone(),
		namespace: Namespace::Value,
		name: info.name.to_vec(),
		kind: kinds::VAR,
		visibility: visibility_for(node, state.source),
		signature: type_signature(type_node, info.pointer_depth, state.source),
		position: Some(node_position(info.name_node)),
		call_name: Vec::new(),
		call_arity: None,
	});
}

fn typedef_defs(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	specifier_member_defs_of(state, node, scope);
	let anonymous_owner = anonymous_typedef_owner(state, node, scope);
	if let (Some(owner), Some(specifier)) = (&anonymous_owner, node.child_by_field_name("type")) {
		specifier_member_defs(state, specifier, scope, Some(owner.clone()));
	}
}

// `typedef struct { ... } name;` hangs its members under type:name since the
// anonymous struct has no moniker of its own.
fn anonymous_typedef_owner(
	state: &mut CDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<Moniker> {
	let specifier = node.child_by_field_name("type")?;
	if !matches!(
		specifier.kind(),
		"struct_specifier" | "union_specifier" | "enum_specifier"
	) {
		return None;
	}
	if specifier.child_by_field_name("name").is_some()
		|| specifier.child_by_field_name("body").is_none()
	{
		return None;
	}
	let declarator = typedef_declarators(node).into_iter().next()?;
	let info = declarator_info(declarator, state.source)?;
	Some(extend_segment(scope, kinds::TYPE, info.name))
}

fn specifier_member_defs_of(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(specifier) = node.child_by_field_name("type") else {
		return;
	};
	specifier_member_defs(state, specifier, scope, None);
}

fn specifier_member_defs(
	state: &mut CDiscover<'_>,
	specifier: Node<'_>,
	scope: &Moniker,
	owner_override: Option<Moniker>,
) {
	let named_owner = || {
		let kind = match specifier.kind() {
			"struct_specifier" | "union_specifier" => kinds::STRUCT,
			"enum_specifier" => kinds::ENUM,
			_ => return None,
		};
		let name_node = specifier.child_by_field_name("name")?;
		Some(extend_segment(
			scope,
			kind,
			node_slice(name_node, state.source),
		))
	};
	let Some(owner) = owner_override
		.or_else(named_owner)
		.or_else(|| matches!(specifier.kind(), "enum_specifier").then(|| scope.clone()))
	else {
		return;
	};
	let Some(body) = specifier.child_by_field_name("body") else {
		return;
	};
	match specifier.kind() {
		"struct_specifier" | "union_specifier" => struct_field_defs(state, body, &owner),
		"enum_specifier" => enum_constant_defs(state, body, &owner),
		_ => {}
	}
}

fn struct_field_defs(state: &mut CDiscover<'_>, body: Node<'_>, owner: &Moniker) {
	for field in named_children(body) {
		if field.kind() != "field_declaration" {
			continue;
		}
		let type_node = field.child_by_field_name("type");
		if let Some(name_node) = recovered_identifier(field) {
			let info = DeclaratorInfo {
				name_node,
				name: node_slice(name_node, state.source),
				pointer_depth: 0,
				is_function: false,
				parameters: None,
			};
			field_def(state, owner, type_node, &info);
			continue;
		}
		let mut cursor = field.walk();
		for declarator in field.children_by_field_name("declarator", &mut cursor) {
			let Some(info) = declarator_info(declarator, state.source) else {
				continue;
			};
			field_def(state, owner, type_node, &info);
		}
	}
}

fn field_def(
	state: &mut CDiscover<'_>,
	owner: &Moniker,
	type_node: Option<Node<'_>>,
	info: &DeclaratorInfo<'_>,
) {
	let moniker = extend_segment(owner, kinds::FIELD, info.name);
	let ty = type_node
		.and_then(|ty| type_expr_for(state, ty, info.pointer_depth))
		.unwrap_or(TypeExpr::Unknown);
	if let Some(_target) = ty.receiver_owner() {
		push_typed_as_ref(state, &moniker, &ty, info.name_node);
	}
	state
		.field_types
		.insert((owner.clone(), info.name.to_vec()), ty);
	state.push_def(DiscoveredDef {
		moniker,
		parent: owner.clone(),
		namespace: Namespace::Value,
		name: info.name.to_vec(),
		kind: kinds::FIELD,
		visibility: kinds::VIS_PUBLIC,
		signature: type_signature(type_node, info.pointer_depth, state.source),
		position: Some(node_position(info.name_node)),
		call_name: Vec::new(),
		call_arity: None,
	});
}

fn enum_constant_defs(state: &mut CDiscover<'_>, body: Node<'_>, owner: &Moniker) {
	for enumerator in named_children(body) {
		if enumerator.kind() != "enumerator" {
			continue;
		}
		let Some(name_node) = enumerator.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, state.source);
		if name.is_empty() {
			continue;
		}
		state.push_def(DiscoveredDef {
			moniker: extend_segment(owner, kinds::ENUM_CONSTANT, name),
			parent: owner.clone(),
			namespace: Namespace::Value,
			name: name.to_vec(),
			kind: kinds::ENUM_CONSTANT,
			visibility: kinds::VIS_PUBLIC,
			signature: Vec::new(),
			position: Some(node_position(enumerator)),
			call_name: Vec::new(),
			call_arity: None,
		});
	}
}

fn object_macro_def(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	if name.is_empty() || is_header_guard_define(node, name, state.source) {
		return;
	}
	state.push_def(DiscoveredDef {
		moniker: extend_segment(scope, kinds::CONST, name),
		parent: scope.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::CONST,
		visibility: kinds::VIS_PUBLIC,
		signature: Vec::new(),
		position: Some(node_position(node)),
		call_name: Vec::new(),
		call_arity: None,
	});
}

fn is_header_guard_define(node: Node<'_>, name: &[u8], source: &[u8]) -> bool {
	let mut ancestor = node.parent();
	let mut guard = None;
	while let Some(current) = ancestor {
		if current.kind() == "preproc_ifdef" {
			guard = Some(current);
			break;
		}
		ancestor = current.parent();
	}
	let Some(parent) = guard else {
		return false;
	};
	let Some(guard_name) = parent.child_by_field_name("name") else {
		return false;
	};
	parent
		.child(0)
		.is_some_and(|directive| directive.kind() == "#ifndef")
		&& node_slice(guard_name, source) == name
}

fn function_macro_def(state: &mut CDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let name = node_slice(name_node, state.source);
	if name.is_empty() {
		return;
	}
	let slots = macro_parameter_slots(node, state.source);
	state
		.macros
		.insert(name.to_vec(), callable_segment_slots(name, &slots));
	state.push_def(DiscoveredDef {
		moniker: extend_callable_slots(scope, kinds::MACRO, name, &slots),
		parent: scope.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::MACRO,
		visibility: kinds::VIS_PUBLIC,
		signature: callable_signature(&slots),
		position: Some(node_position(node)),
		call_name: name.to_vec(),
		call_arity: Some(slots.len()),
	});
}

fn macro_parameter_slots(node: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let Some(params) = node.child_by_field_name("parameters") else {
		return Vec::new();
	};
	named_children(params)
		.filter(|param| matches!(param.kind(), "identifier"))
		.map(|param| CallableSlot {
			name: node_slice(param, source).to_vec(),
			r#type: Vec::new(),
		})
		.collect()
}

fn param_defs(state: &mut CDiscover<'_>, params: Node<'_>, callable: &Moniker) {
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
		push_binding_def(
			state,
			callable,
			kinds::PARAM,
			info.name,
			node_position(info.name_node),
		);
	}
}

fn body_defs(state: &mut CDiscover<'_>, node: Node<'_>, callable: &Moniker) {
	if state.deep && node.kind() == "declaration" {
		for declarator in declaration_declarators(node) {
			if let Some(info) = declarator_info(declarator, state.source) {
				if !info.is_function {
					push_binding_def(
						state,
						callable,
						kinds::LOCAL,
						info.name,
						node_position(info.name_node),
					);
				}
			}
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

fn push_binding_def(
	state: &mut CDiscover<'_>,
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

fn push_typed_as_ref(state: &mut CDiscover<'_>, source: &Moniker, ty: &TypeExpr, node: Node<'_>) {
	let Some(target) = ty.receiver_owner().cloned() else {
		return;
	};
	state.push_ref(ResolvedRef {
		source: source.clone(),
		target,
		kind: kinds::TYPED_AS,
		position: Some(node_position(node)),
		confidence: kinds::CONF_RESOLVED,
		hints: RefHints::default(),
	});
}

fn callable_signature(slots: &[CallableSlot]) -> Vec<u8> {
	join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>())
}

fn type_signature(type_node: Option<Node<'_>>, pointer_depth: usize, source: &[u8]) -> Vec<u8> {
	let mut signature = type_node
		.and_then(|ty| ty.utf8_text(source).ok())
		.map(normalize_type_text)
		.unwrap_or_default();
	signature.extend(std::iter::repeat_n(b'*', pointer_depth));
	signature
}

struct PendingComment {
	start_byte: u32,
	end_byte: u32,
	end_row: usize,
}

fn extend_or_flush_comment(
	state: &mut CDiscover<'_>,
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

fn flush_comment(state: &mut CDiscover<'_>, pending: &mut Option<PendingComment>, scope: &Moniker) {
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
