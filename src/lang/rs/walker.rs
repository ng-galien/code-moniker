use std::cell::RefCell;
use std::collections::HashSet;

use tree_sitter::Node;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use super::canonicalize::{
	closure_param_types, extend_callable_typed, extend_segment, extend_segment_u32,
	function_param_types, impl_type_name, node_position,
};
use super::kinds;

pub(super) struct Walker<'src> {
	pub(super) source_bytes: &'src [u8],
	pub(super) module: Moniker,
	pub(super) local_mods: HashSet<String>,
	pub(super) deep: bool,
	pub(super) local_scope: RefCell<Vec<HashSet<&'src [u8]>>>,
	pub(super) type_params: RefCell<Vec<HashSet<&'src [u8]>>>,
}

impl<'src> Walker<'src> {
	pub(super) fn push_local_scope(&self) {
		self.local_scope.borrow_mut().push(HashSet::new());
	}
	pub(super) fn pop_local_scope(&self) {
		self.local_scope.borrow_mut().pop();
	}
	pub(super) fn record_local(&self, name: &'src [u8]) {
		if let Some(top) = self.local_scope.borrow_mut().last_mut() {
			top.insert(name);
		}
	}
	pub(super) fn is_local_in_scope(&self, name: &[u8]) -> bool {
		self.local_scope
			.borrow()
			.iter()
			.any(|frame| frame.contains(name))
	}
	pub(super) fn push_type_params(&self, params: HashSet<&'src [u8]>) {
		self.type_params.borrow_mut().push(params);
	}
	pub(super) fn pop_type_params(&self) {
		self.type_params.borrow_mut().pop();
	}
	pub(super) fn is_type_param_in_scope(&self, name: &[u8]) -> bool {
		self.type_params
			.borrow()
			.iter()
			.any(|frame| frame.contains(name))
	}
}

pub(super) fn collect_local_mods(root: Node<'_>, source: &[u8]) -> HashSet<String> {
	let mut out = HashSet::new();
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() == "mod_item"
			&& let Some(name) = child.child_by_field_name("name")
			&& let Ok(s) = name.utf8_text(source)
		{
			out.insert(s.to_string());
		}
	}
	out
}

impl<'src> Walker<'src> {
	pub(super) fn walk(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			self.dispatch(child, scope, graph);
		}
	}

	pub(super) fn dispatch(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		match node.kind() {
			"struct_item" => self.handle_simple_def(node, scope, graph, kinds::STRUCT),
			"enum_item" => self.handle_simple_def(node, scope, graph, kinds::ENUM),
			"trait_item" => self.handle_trait(node, scope, graph),
			"type_item" => self.handle_simple_def(node, scope, graph, kinds::TYPE),
			"function_item" => self.handle_function(node, scope, graph),
			"impl_item" => self.handle_impl(node, scope, graph),
			"use_declaration" => self.handle_use(node, scope, graph),
			"let_declaration" => self.handle_let(node, scope, graph),
			"call_expression" => self.handle_call(node, scope, graph),
			"macro_invocation" => self.handle_macro(node, scope, graph),
			"struct_expression" => self.handle_struct_literal(node, scope, graph),
			"field_declaration" => self.handle_field_declaration(node, scope, graph),
			"attribute_item" => self.handle_attribute(node, scope, graph),
			"identifier" => self.handle_identifier_read(node, scope, graph),
			"scoped_identifier" => self.handle_scoped_read(node, scope, graph),
			"line_comment" | "block_comment" => self.handle_comment(node, scope, graph),
			"mod_item" => {}
			_ => self.walk(node, scope, graph),
		}
	}

	fn handle_simple_def(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		kind: &[u8],
	) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(scope, kind, name.as_bytes());
		let _ = graph.add_def(m.clone(), kind, scope, Some(node_position(node)));
		let pushed = self.push_type_params_from(node);
		self.walk(node, &m, graph);
		if pushed {
			self.pop_type_params();
		}
	}

	fn handle_trait(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(scope, kinds::TRAIT, name.as_bytes());
		let _ = graph.add_def(m.clone(), kinds::TRAIT, scope, Some(node_position(node)));
		let pushed = self.push_type_params_from(node);
		if let Some(bounds) = node.child_by_field_name("bounds") {
			self.handle_trait_bounds_extends(&m, bounds, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
		if pushed {
			self.pop_type_params();
		}
	}

	fn push_type_params_from(&self, node: Node<'_>) -> bool {
		let Some(tp) = node.child_by_field_name("type_parameters") else {
			return false;
		};
		let mut names: HashSet<&'src [u8]> = HashSet::new();
		let mut cursor = tp.walk();
		for child in tp.named_children(&mut cursor) {
			if child.kind() == "type_parameter"
				&& let Some(name_node) = child.child_by_field_name("name")
			{
				names.insert(&self.source_bytes[name_node.start_byte()..name_node.end_byte()]);
			}
		}
		if names.is_empty() {
			return false;
		}
		self.push_type_params(names);
		true
	}

	fn handle_function(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let kind = if is_type_scope(scope) {
			kinds::METHOD
		} else {
			kinds::FN
		};
		let types = function_param_types(node, self.source_bytes);
		let m = extend_callable_typed(scope, kind, name.as_bytes(), &types);
		let _ = graph.add_def(m.clone(), kind, scope, Some(node_position(node)));
		let type_params_pushed = self.push_type_params_from(node);
		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			self.record_param_names(params);
			self.emit_param_type_refs(params, &m, graph);
			if self.deep {
				self.emit_params(params, &m, graph);
			}
		}
		if let Some(rt) = node.child_by_field_name("return_type") {
			self.emit_uses_type_walk(rt, &m, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
		self.pop_local_scope();
		if type_params_pushed {
			self.pop_type_params();
		}
	}

	fn record_param_names(&self, params: Node<'_>) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"self_parameter" => self.record_local(b"self"),
				"parameter" => {
					if let Some(pattern) = child.child_by_field_name("pattern") {
						self.record_pattern_names(pattern);
					}
				}
				_ => self.record_pattern_names(child),
			}
		}
	}

	fn record_pattern_names(&self, pattern: Node<'_>) {
		visit_pattern_identifiers(pattern, &mut |ident| {
			let bytes = &self.source_bytes[ident.start_byte()..ident.end_byte()];
			self.record_local(bytes);
		});
	}

	fn emit_param_type_refs(&self, params: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			if child.kind() == "parameter"
				&& let Some(ty) = child.child_by_field_name("type")
			{
				self.emit_uses_type_walk(ty, callable, graph);
			}
		}
	}

	fn emit_params(&self, params: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"self_parameter" => {
					self.emit_pattern_leaf(callable, kinds::PARAM, b"self", child, graph)
				}
				"parameter" => {
					if let Some(pattern) = child.child_by_field_name("pattern") {
						self.emit_pattern_defs(pattern, callable, kinds::PARAM, child, graph);
					}
				}
				_ => self.emit_pattern_defs(child, callable, kinds::PARAM, child, graph),
			}
		}
	}

	fn emit_pattern_defs(
		&self,
		pattern: Node<'_>,
		callable: &Moniker,
		kind: &[u8],
		anchor: Node<'_>,
		graph: &mut CodeGraph,
	) {
		visit_pattern_identifiers(pattern, &mut |ident| {
			if let Ok(name) = ident.utf8_text(self.source_bytes) {
				self.emit_pattern_leaf(callable, kind, name.as_bytes(), anchor, graph);
			}
		});
	}

	fn emit_pattern_leaf(
		&self,
		callable: &Moniker,
		kind: &[u8],
		name: &[u8],
		anchor: Node<'_>,
		graph: &mut CodeGraph,
	) {
		let m = extend_segment(callable, kind, name);
		let _ = graph.add_def(m, kind, callable, Some(node_position(anchor)));
	}

	fn handle_comment(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let m = extend_segment_u32(scope, kinds::COMMENT, node.start_byte() as u32);
		let _ = graph.add_def(m, kinds::COMMENT, scope, Some(node_position(node)));
	}

	fn handle_let(&self, node: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let Some(pattern) = node.child_by_field_name("pattern") else {
			return;
		};
		self.record_pattern_names(pattern);
		if self.deep {
			self.emit_pattern_defs(pattern, callable, kinds::LOCAL, node, graph);
		}
		if let Some(ty) = node.child_by_field_name("type") {
			self.emit_uses_type_walk(ty, callable, graph);
		}
		let Some(value) = node.child_by_field_name("value") else {
			return;
		};
		if value.kind() == "closure_expression"
			&& let Some(bind_name) = first_identifier(pattern, self.source_bytes)
		{
			self.record_local(bind_name.as_bytes());
			self.emit_named_closure(value, callable, bind_name.as_bytes(), graph);
			return;
		}
		self.dispatch(value, callable, graph);
	}

	fn emit_named_closure(
		&self,
		closure: Node<'_>,
		callable: &Moniker,
		name: &[u8],
		graph: &mut CodeGraph,
	) {
		let types = closure_param_types(closure, self.source_bytes);
		let m = extend_callable_typed(callable, kinds::FN, name, &types);
		let _ = graph.add_def(m.clone(), kinds::FN, callable, Some(node_position(closure)));
		self.push_local_scope();
		if let Some(params) = closure.child_by_field_name("parameters") {
			self.record_param_names(params);
			if self.deep {
				self.emit_params(params, &m, graph);
			}
		}
		if let Some(body) = closure.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
		self.pop_local_scope();
	}

	fn handle_impl(&self, node: Node<'_>, _scope: &Moniker, graph: &mut CodeGraph) {
		let Some(type_node) = node.child_by_field_name("type") else {
			return;
		};
		let Some(type_name) = impl_type_name(type_node, self.source_bytes) else {
			return;
		};
		let type_moniker = extend_segment(&self.module, kinds::STRUCT, type_name.as_bytes());
		self.handle_impl_trait_for(node, &type_moniker, graph);
		let Some(body) = node.child_by_field_name("body") else {
			return;
		};
		self.walk(body, &type_moniker, graph);
	}

	pub(super) fn field_text(&self, node: Node<'_>, field: &str) -> Option<&'src str> {
		node.child_by_field_name(field)?
			.utf8_text(self.source_bytes)
			.ok()
	}
}

fn visit_pattern_identifiers(pattern: Node<'_>, leaf: &mut impl FnMut(Node<'_>)) {
	match pattern.kind() {
		"identifier" => leaf(pattern),
		"_" => {}
		_ => {
			let mut cursor = pattern.walk();
			for inner in pattern.named_children(&mut cursor) {
				visit_pattern_identifiers(inner, leaf);
			}
		}
	}
}

fn is_type_scope(scope: &Moniker) -> bool {
	matches!(
		scope.last_kind().as_deref(),
		Some(b"struct") | Some(b"trait") | Some(b"enum")
	)
}

fn first_identifier<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
	if node.kind() == "identifier" {
		return node.utf8_text(source).ok();
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if let Some(found) = first_identifier(child, source) {
			return Some(found);
		}
	}
	None
}
