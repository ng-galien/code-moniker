use tree_sitter::Node;

use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::callable::{
	CallableSlot, callable_segment_slots, extend_callable_slots, extend_segment,
};
use crate::lang::sdk::{DiscoveredDef, Namespace, TypeExpr};
use crate::lang::tree_util::{node_position, node_slice};

use super::super::kinds;
use super::discover::JavaDiscover;
use super::syntax::{named_children, path_pieces};
use super::type_resolution::type_expr;

#[derive(Clone, Copy, Default)]
pub(super) struct LombokType {
	getter: bool,
	setter: bool,
	wither: bool,
	builder: bool,
	logger: Option<&'static [&'static str]>,
}

pub(super) fn emit_generated_members(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) {
	let Some(kind) = type_kind(node.kind()) else {
		for child in named_children(node) {
			emit_generated_members(state, child, scope);
		}
		return;
	};
	let Some(name_node) = node.child_by_field_name("name") else {
		return;
	};
	let owner = extend_segment(scope, kind, node_slice(name_node, state.source));
	let semantics = type_semantics(state, node);
	if let Some(semantics) = semantics {
		emit_type_members(state, node, &owner, semantics);
	}
	let Some(body) = node.child_by_field_name("body") else {
		return;
	};
	for child in named_children(body) {
		if child.kind() == "field_declaration" {
			emit_fields(state, child, &owner, semantics);
		} else {
			emit_generated_members(state, child, &owner);
		}
	}
}

fn type_semantics(state: &JavaDiscover<'_>, node: Node<'_>) -> Option<LombokType> {
	let mut semantics = LombokType::default();
	for annotation in lombok_annotations(state, node) {
		match annotation.as_slice() {
			b"Data" => {
				semantics.getter = true;
				semantics.setter = true;
			}
			b"Value" => semantics.getter = true,
			b"Getter" => semantics.getter = true,
			b"Setter" => semantics.setter = true,
			b"With" | b"Wither" | b"WithBy" => semantics.wither = true,
			b"Builder" | b"SuperBuilder" => semantics.builder = true,
			b"Slf4j" => semantics.logger = Some(&["org", "slf4j", "Logger"]),
			b"XSlf4j" => semantics.logger = Some(&["org", "slf4j", "ext", "XLogger"]),
			b"Log4j" => semantics.logger = Some(&["org", "apache", "log4j", "Logger"]),
			b"Log4j2" => semantics.logger = Some(&["org", "apache", "logging", "log4j", "Logger"]),
			b"CommonsLog" => {
				semantics.logger = Some(&["org", "apache", "commons", "logging", "Log"])
			}
			b"JBossLog" => semantics.logger = Some(&["org", "jboss", "logging", "Logger"]),
			b"Flogger" => {
				semantics.logger = Some(&["com", "google", "common", "flogger", "FluentLogger"])
			}
			b"Log" => semantics.logger = Some(&["java", "util", "logging", "Logger"]),
			_ => {}
		}
	}
	(semantics.getter
		|| semantics.setter
		|| semantics.wither
		|| semantics.builder
		|| semantics.logger.is_some())
	.then_some(semantics)
}

fn field_semantics(state: &JavaDiscover<'_>, node: Node<'_>) -> Option<LombokType> {
	let mut semantics = LombokType::default();
	for annotation in lombok_annotations(state, node) {
		match annotation.as_slice() {
			b"Getter" => semantics.getter = true,
			b"Setter" => semantics.setter = true,
			b"With" | b"Wither" | b"WithBy" => semantics.wither = true,
			_ => {}
		}
	}
	(semantics.getter || semantics.setter || semantics.wither).then_some(semantics)
}

fn emit_type_members(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	owner: &Moniker,
	semantics: LombokType,
) {
	if semantics.builder {
		push_method(
			state,
			owner,
			b"builder",
			0,
			Some(TypeExpr::chain_continuation(owner.clone())),
			node,
		);
		push_method(
			state,
			owner,
			b"toBuilder",
			0,
			Some(TypeExpr::chain_continuation(owner.clone())),
			node,
		);
		push_method(
			state,
			owner,
			b"build",
			0,
			Some(TypeExpr::resolved(owner.clone())),
			node,
		);
	}
	if let Some(logger) = semantics.logger {
		let target = external_type(owner, logger);
		let moniker = extend_segment(owner, kinds::FIELD, b"log");
		if state.defs.iter().any(|def| def.moniker == moniker) {
			return;
		}
		state.field_types.insert(
			(owner.clone(), b"log".to_vec()),
			TypeExpr::external_opaque(target),
		);
		state.push_def(DiscoveredDef {
			moniker,
			parent: owner.clone(),
			namespace: Namespace::Value,
			name: b"log".to_vec(),
			kind: kinds::FIELD,
			visibility: kinds::VIS_PRIVATE,
			signature: Vec::new(),
			position: Some(node_position(node)),
			call_name: Vec::new(),
			call_arity: None,
		});
	}
}

#[allow(clippy::too_many_arguments)]
fn emit_field_members(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	owner: &Moniker,
	name: &[u8],
	ty: Option<&TypeExpr>,
	type_semantics: Option<LombokType>,
	field_semantics: Option<LombokType>,
) {
	let type_semantics = type_semantics.unwrap_or_default();
	let field_semantics = field_semantics.unwrap_or_default();
	let getter = type_semantics.getter || field_semantics.getter;
	let setter = (type_semantics.setter || field_semantics.setter) && !has_modifier(node, "final");
	let wither = type_semantics.wither || field_semantics.wither;
	if getter {
		let prefix: &[u8] = if primitive_boolean(node, state.source) {
			b"is"
		} else {
			b"get"
		};
		let method = property_method_name(prefix, name);
		push_method(state, owner, &method, 0, ty.cloned(), node);
	}
	if setter {
		let method = property_method_name(b"set", name);
		push_method(state, owner, &method, 1, None, node);
	}
	if wither {
		let method = property_method_name(b"with", name);
		push_method(
			state,
			owner,
			&method,
			1,
			Some(TypeExpr::resolved(owner.clone())),
			node,
		);
	}
	if type_semantics.builder {
		push_method(
			state,
			owner,
			name,
			1,
			Some(TypeExpr::chain_continuation(owner.clone())),
			node,
		);
	}
}

fn emit_fields(
	state: &mut JavaDiscover<'_>,
	node: Node<'_>,
	owner: &Moniker,
	type_semantics: Option<LombokType>,
) {
	let ty = node
		.child_by_field_name("type")
		.and_then(|node| type_expr(state, node, owner));
	let field_semantics = field_semantics(state, node);
	if type_semantics.is_none() && field_semantics.is_none() {
		return;
	}
	for declarator in named_children(node).filter(|child| child.kind() == "variable_declarator") {
		let Some(name_node) = declarator.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, state.source);
		if !name.is_empty() {
			emit_field_members(
				state,
				node,
				owner,
				name,
				ty.as_ref(),
				type_semantics,
				field_semantics,
			);
		}
	}
}

fn push_method(
	state: &mut JavaDiscover<'_>,
	owner: &Moniker,
	name: &[u8],
	arity: usize,
	return_type: Option<TypeExpr>,
	node: Node<'_>,
) {
	let slots = (0..arity)
		.map(|_| CallableSlot::default())
		.collect::<Vec<_>>();
	let moniker = extend_callable_slots(owner, kinds::METHOD, name, &slots);
	if state.defs.iter().any(|def| def.moniker == moniker) {
		return;
	}
	state.callables.insert(
		(owner.clone(), name.to_vec(), arity),
		callable_segment_slots(name, &slots),
	);
	if let Some(return_type) = return_type {
		state
			.return_types
			.insert((owner.clone(), name.to_vec(), arity), return_type);
	}
	state.push_def(DiscoveredDef {
		moniker,
		parent: owner.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::METHOD,
		visibility: kinds::VIS_PUBLIC,
		signature: Vec::new(),
		position: Some(node_position(node)),
		call_name: name.to_vec(),
		call_arity: Some(arity),
	});
}

fn lombok_annotations(state: &JavaDiscover<'_>, node: Node<'_>) -> Vec<Vec<u8>> {
	let mut out = Vec::new();
	for modifiers in named_children(node).filter(|child| child.kind() == "modifiers") {
		for annotation in named_children(modifiers)
			.filter(|child| matches!(child.kind(), "marker_annotation" | "annotation"))
		{
			let Some(name_node) = annotation.child_by_field_name("name") else {
				continue;
			};
			let path = path_pieces(name_node, state.source);
			let Some(name) = path.last() else {
				continue;
			};
			let qualified = path.first().is_some_and(|piece| piece == b"lombok");
			let imported =
				state.imports.iter().any(|import| {
					import.name == *name
						&& import.target.as_view().segments().any(|segment| {
							segment.kind == kinds::PACKAGE && segment.name == b"lombok"
						})
				});
			if qualified || imported {
				out.push(name.clone());
			}
		}
	}
	out
}

fn property_method_name(prefix: &[u8], field: &[u8]) -> Vec<u8> {
	let mut out = Vec::with_capacity(prefix.len() + field.len());
	out.extend_from_slice(prefix);
	out.extend_from_slice(field);
	if let Some(first) = out.get_mut(prefix.len()) {
		first.make_ascii_uppercase();
	}
	out
}

fn primitive_boolean(node: Node<'_>, source: &[u8]) -> bool {
	node.child_by_field_name("type")
		.is_some_and(|ty| node_slice(ty, source) == b"boolean")
}

fn has_modifier(node: Node<'_>, modifier_kind: &str) -> bool {
	named_children(node)
		.filter(|child| child.kind() == "modifiers")
		.any(|child| {
			let mut cursor = child.walk();
			child
				.children(&mut cursor)
				.any(|modifier| modifier.kind() == modifier_kind)
		})
}

fn external_type(owner: &Moniker, path: &[&str]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(owner.as_view().project());
	if let Some((head, tail)) = path.split_first() {
		builder.segment(kinds::EXTERNAL_PKG, head.as_bytes());
		for piece in tail {
			builder.segment(kinds::PATH, piece.as_bytes());
		}
	}
	builder.build()
}

fn type_kind(kind: &str) -> Option<&'static [u8]> {
	match kind {
		"class_declaration" => Some(kinds::CLASS),
		"interface_declaration" => Some(kinds::INTERFACE),
		"enum_declaration" => Some(kinds::ENUM),
		"record_declaration" => Some(kinds::RECORD),
		"annotation_type_declaration" => Some(kinds::ANNOTATION_TYPE),
		_ => None,
	}
}
