use tree_sitter::Node;

use crate::core::code_graph::Position;
use crate::core::moniker::Moniker;
use crate::lang::callable::{
	CallableSlot, callable_segment_slots, extend_segment, extend_segment_u32,
};
use crate::lang::sdk::{DiscoveredDef, Namespace};
use crate::lang::tree_util::{find_descendant, node_position, node_slice};

use super::super::kinds;
use super::syntax::path_pieces;

pub(super) struct DefEnv<'a> {
	pub root: &'a Moniker,
	pub source: &'a [u8],
}

pub(super) fn simple_def(
	env: DefEnv<'_>,
	node: Node<'_>,
	scope: &Moniker,
	kind: &'static [u8],
	namespace: Namespace,
	trait_impl: bool,
) -> Option<DiscoveredDef> {
	let name_node = definition_name_node(node)?;
	let name = node_slice(name_node, env.source);
	Some(def_record(DefRecordInput {
		moniker: extend_segment(scope, kind, name),
		parent: scope.clone(),
		namespace,
		name: name.to_vec(),
		kind,
		visibility: def_visibility(kind, node, env.source, scope, trait_impl),
		signature: Vec::new(),
		position: Some(node_position(node)),
		call_name: Vec::new(),
		call_arity: None,
	}))
}

fn definition_name_node(node: Node<'_>) -> Option<Node<'_>> {
	node.child_by_field_name("name").or_else(|| {
		let mut cursor = node.walk();
		node.named_children(&mut cursor)
			.find(|child| child.kind() == "identifier" || child.kind() == "type_identifier")
	})
}

pub(super) fn callable_def(env: DefEnv<'_>, input: CallableDefInput<'_>) -> DiscoveredDef {
	let name = node_slice(input.name_node, env.source);
	let signature = if input.kind == kinds::TEST {
		test_signature(input.node, env.source, name)
	} else {
		Vec::new()
	};
	def_record(DefRecordInput {
		moniker: extend_segment(
			input.scope,
			input.kind,
			&callable_segment_slots(name, input.slots),
		),
		parent: input.scope.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: input.kind,
		visibility: visibility_of(input.node, env.source, input.scope, input.trait_impl),
		position: Some(node_position(input.node)),
		signature,
		call_name: name.to_vec(),
		call_arity: Some(input.slots.len()),
	})
}

pub(super) fn reexport_path_def(
	_env: DefEnv<'_>,
	node: Node<'_>,
	scope: &Moniker,
	name: &[u8],
) -> DiscoveredDef {
	def_record(DefRecordInput {
		moniker: extend_segment(scope, kinds::PATH, name),
		parent: scope.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::PATH,
		visibility: kinds::VIS_PUBLIC,
		position: Some(node_position(node)),
		signature: Vec::new(),
		call_name: Vec::new(),
		call_arity: None,
	})
}

pub(super) fn proptest_def(
	node: Node<'_>,
	scope: &Moniker,
	name: &[u8],
	param_name: &[u8],
	ignore: Option<&str>,
) -> DiscoveredDef {
	let mut moniker_name = name.to_vec();
	moniker_name.extend_from_slice(b"(");
	moniker_name.extend_from_slice(param_name);
	moniker_name.extend_from_slice(b")");
	def_record(DefRecordInput {
		moniker: extend_segment(scope, kinds::TEST, &moniker_name),
		parent: scope.clone(),
		namespace: Namespace::Value,
		name: name.to_vec(),
		kind: kinds::TEST,
		visibility: kinds::VIS_PRIVATE,
		position: Some(node_position(node)),
		signature: proptest_signature(name, ignore),
		call_name: Vec::new(),
		call_arity: None,
	})
}

pub(super) struct CallableDefInput<'a> {
	pub node: Node<'a>,
	pub name_node: Node<'a>,
	pub scope: &'a Moniker,
	pub kind: &'static [u8],
	pub slots: &'a [CallableSlot],
	pub trait_impl: bool,
}

pub(super) fn enum_constant_def(
	env: DefEnv<'_>,
	node: Node<'_>,
	name_node: Node<'_>,
	enum_moniker: &Moniker,
) -> DiscoveredDef {
	let name = node_slice(name_node, env.source);
	child_value_def(node, enum_moniker, kinds::ENUM_CONSTANT, name.to_vec())
}

pub(super) fn synthetic_enum_constant_def(
	node: Node<'_>,
	enum_moniker: &Moniker,
	name: Vec<u8>,
) -> DiscoveredDef {
	child_value_def(node, enum_moniker, kinds::ENUM_CONSTANT, name)
}

pub(super) fn inferred_struct_def(
	env: DefEnv<'_>,
	node: Node<'_>,
	moniker: &Moniker,
	name: &str,
) -> DiscoveredDef {
	def_record(DefRecordInput {
		moniker: moniker.clone(),
		parent: env.root.clone(),
		namespace: Namespace::Type,
		name: name.as_bytes().to_vec(),
		kind: kinds::STRUCT,
		visibility: kinds::VIS_NONE,
		signature: Vec::new(),
		position: Some(node_position(node)),
		call_name: Vec::new(),
		call_arity: None,
	})
}

pub(super) fn synthetic_lang_enum_def(
	node: Node<'_>,
	scope: &Moniker,
	enum_moniker: &Moniker,
) -> DiscoveredDef {
	def_record(DefRecordInput {
		moniker: enum_moniker.clone(),
		parent: scope.clone(),
		namespace: Namespace::Type,
		name: b"Lang".to_vec(),
		kind: kinds::ENUM,
		visibility: kinds::VIS_NONE,
		signature: Vec::new(),
		position: Some(node_position(node)),
		call_name: Vec::new(),
		call_arity: None,
	})
}

pub(super) fn nested_type_def(
	env: DefEnv<'_>,
	node: Node<'_>,
	function: &Moniker,
	kind: &'static [u8],
	name_node: Node<'_>,
) -> DiscoveredDef {
	let name = node_slice(name_node, env.source);
	def_record(DefRecordInput {
		moniker: extend_segment(function, kind, name),
		parent: function.clone(),
		namespace: Namespace::Type,
		name: name.to_vec(),
		kind,
		visibility: visibility_of(node, env.source, function, false),
		signature: Vec::new(),
		position: Some(node_position(node)),
		call_name: Vec::new(),
		call_arity: None,
	})
}

pub(super) fn local_binding_def(
	function: &Moniker,
	kind: &'static [u8],
	name: Vec<u8>,
	position: Option<Position>,
) -> DiscoveredDef {
	def_record(DefRecordInput {
		moniker: extend_segment(function, kind, &name),
		parent: function.clone(),
		namespace: Namespace::Value,
		name,
		kind,
		visibility: kinds::VIS_NONE,
		signature: Vec::new(),
		position,
		call_name: Vec::new(),
		call_arity: None,
	})
}

pub(super) fn comment_def(scope: &Moniker, start_byte: u32, end_byte: u32) -> DiscoveredDef {
	def_record(DefRecordInput {
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
	})
}

fn child_value_def(
	node: Node<'_>,
	parent: &Moniker,
	kind: &'static [u8],
	name: Vec<u8>,
) -> DiscoveredDef {
	def_record(DefRecordInput {
		moniker: extend_segment(parent, kind, &name),
		parent: parent.clone(),
		namespace: Namespace::Value,
		name,
		kind,
		visibility: kinds::VIS_NONE,
		signature: Vec::new(),
		position: Some(node_position(node)),
		call_name: Vec::new(),
		call_arity: None,
	})
}

fn def_record(input: DefRecordInput) -> DiscoveredDef {
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

fn test_signature(node: Node<'_>, source: &[u8], name: &[u8]) -> Vec<u8> {
	let ignore = test_ignore_reason(node, source);
	let mut signature = b"framework=rust-test;enabled=true;display=".to_vec();
	if ignore.is_some() {
		signature = b"framework=rust-test;enabled=false;display=".to_vec();
	}
	signature.extend_from_slice(name);
	if let Some(reason) = ignore.filter(|reason| !reason.is_empty()) {
		signature.extend_from_slice(b";ignore=");
		signature.extend_from_slice(reason.as_bytes());
	}
	signature
}

fn test_ignore_reason(node: Node<'_>, source: &[u8]) -> Option<String> {
	let mut sibling = node.prev_named_sibling();
	while let Some(previous) = sibling {
		if previous.kind() != "attribute_item" {
			break;
		}
		if let Some(reason) = ignore_reason(previous, source) {
			return Some(reason);
		}
		sibling = previous.prev_named_sibling();
	}
	None
}

fn ignore_reason(attribute: Node<'_>, source: &[u8]) -> Option<String> {
	if path_pieces(attribute, source) != vec![b"ignore".to_vec()] {
		return None;
	}
	find_descendant(attribute, "string_content")
		.map(|reason| String::from_utf8_lossy(node_slice(reason, source)).into_owned())
		.or_else(|| Some(String::new()))
}

fn proptest_signature(name: &[u8], ignore: Option<&str>) -> Vec<u8> {
	let mut signature = if ignore.is_some() {
		b"framework=proptest;enabled=false;display=".to_vec()
	} else {
		b"framework=proptest;enabled=true;display=".to_vec()
	};
	signature.extend_from_slice(name);
	if let Some(reason) = ignore.filter(|reason| !reason.is_empty()) {
		signature.extend_from_slice(b";ignore=");
		signature.extend_from_slice(reason.as_bytes());
	}
	signature
}

struct DefRecordInput {
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

fn visibility_of(
	node: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	trait_impl: bool,
) -> &'static [u8] {
	let mut cursor = node.walk();
	let has_pub = node
		.children(&mut cursor)
		.any(|child| child.kind() == "visibility_modifier" && node_slice(child, source) == b"pub");
	if has_pub || trait_impl || scope.last_kind().as_deref() == Some(kinds::TRAIT) {
		kinds::VIS_PUBLIC
	} else {
		kinds::VIS_PRIVATE
	}
}

fn def_visibility(
	kind: &[u8],
	node: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	trait_impl: bool,
) -> &'static [u8] {
	if kind == kinds::MACRO {
		kinds::VIS_NONE
	} else {
		visibility_of(node, source, scope, trait_impl)
	}
}
