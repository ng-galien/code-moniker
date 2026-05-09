use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs};
use crate::core::moniker::Moniker;

use super::canonicalize::{
	extend_callable_typed, extend_segment, function_param_types, node_position,
};
use super::kinds;
use super::scope::{is_callable_scope, visibility_from_name};

#[derive(Clone, Debug)]
pub(super) struct ImportEntry {
	pub confidence: &'static [u8],
	pub module_prefix: Moniker,
}

pub(super) struct Walker<'src> {
	pub(super) source_bytes: &'src [u8],
	pub(super) module: Moniker,
	pub(super) deep: bool,
	pub(super) local_scope: RefCell<Vec<HashSet<&'src [u8]>>>,
	pub(super) imports: RefCell<HashMap<&'src [u8], ImportEntry>>,
	pub(super) type_table: HashMap<&'src [u8], Moniker>,
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
			"package_clause" => {}
			"import_declaration" => self.handle_import(node, scope, graph),
			"function_declaration" => self.handle_function(node, scope, graph),
			"method_declaration" => self.handle_method(node, scope, graph),
			"type_declaration" => self.handle_type_declaration(node, scope, graph),
			"call_expression" => self.handle_call(node, scope, graph),
			"composite_literal" => self.handle_composite_literal(node, scope, graph),
			"short_var_declaration" => self.handle_short_var_declaration(node, scope, graph),
			"var_declaration" => self.handle_var_declaration(node, scope, graph),
			"range_clause" => self.handle_range_clause(node, scope, graph),
			_ => self.walk(node, scope, graph),
		}
	}

	fn handle_type_declaration(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			match child.kind() {
				"type_spec" => self.handle_type_spec(child, scope, graph),
				"type_alias" => self.handle_type_alias(child, scope, graph),
				_ => {}
			}
		}
	}

	fn handle_type_spec(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let Some(type_node) = node.child_by_field_name("type") else {
			return;
		};
		let kind = match type_node.kind() {
			"struct_type" => kinds::STRUCT,
			"interface_type" => kinds::INTERFACE,
			_ => kinds::TYPE,
		};
		let m = extend_segment(scope, kind, name.as_bytes());
		if scope != &self.module {
			let attrs = DefAttrs {
				visibility: visibility_from_name(name.as_bytes()),
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(m.clone(), kind, scope, Some(node_position(node)), &attrs);
		}
		match type_node.kind() {
			"struct_type" => self.emit_struct_body(type_node, &m, graph),
			"interface_type" => self.emit_interface_body(type_node, &m, graph),
			_ => self.emit_uses_type(type_node, &m, graph),
		}
	}

	fn handle_type_alias(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(scope, kinds::TYPE, name.as_bytes());
		if scope != &self.module {
			let attrs = DefAttrs {
				visibility: visibility_from_name(name.as_bytes()),
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(
				m.clone(),
				kinds::TYPE,
				scope,
				Some(node_position(node)),
				&attrs,
			);
		}
		if let Some(type_node) = node.child_by_field_name("type") {
			self.emit_uses_type(type_node, &m, graph);
		}
	}

	fn handle_method(&self, node: Node<'_>, _scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let Some(receiver) = node.child_by_field_name("receiver") else {
			return;
		};
		let Some(receiver_name) = receiver_type_name(receiver, self.source_bytes) else {
			return;
		};
		let owner = self
			.type_table
			.get(receiver_name)
			.cloned()
			.unwrap_or_else(|| extend_segment(&self.module, kinds::STRUCT, receiver_name));
		let types = function_param_types(node, self.source_bytes);
		let signature = crate::lang::callable::join_bytes_with_comma(&types);
		let m = extend_callable_typed(&owner, kinds::METHOD, name.as_bytes(), &types);
		let attrs = DefAttrs {
			visibility: visibility_from_name(name.as_bytes()),
			signature: &signature,
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::METHOD,
			&owner,
			Some(node_position(node)),
			&attrs,
		);
		self.push_local_scope();
		self.handle_method_receiver(receiver, &m, graph);
		self.handle_callable_params(node, &m, graph);
		self.emit_callable_type_refs(node, &m, graph);
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
		self.pop_local_scope();
	}

	fn handle_callable_params(
		&self,
		callable: Node<'_>,
		callable_m: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(params) = callable.child_by_field_name("parameters") else {
			return;
		};
		self.bind_param_identifiers(params, callable_m, graph);
	}

	fn handle_method_receiver(
		&self,
		receiver: Node<'_>,
		callable_m: &Moniker,
		graph: &mut CodeGraph,
	) {
		self.bind_param_identifiers(receiver, callable_m, graph);
	}

	fn handle_short_var_declaration(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if is_callable_scope(scope, &self.module)
			&& let Some(left) = node.child_by_field_name("left")
		{
			self.bind_locals_in_expression_list(left, scope, graph);
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.dispatch(right, scope, graph);
		}
	}

	fn handle_var_declaration(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let in_callable = is_callable_scope(scope, &self.module);
		let mut cursor = node.walk();
		for spec in node.named_children(&mut cursor) {
			if spec.kind() != "var_spec" {
				continue;
			}
			if let Some(t) = spec.child_by_field_name("type") {
				self.emit_uses_type(t, scope, graph);
			}
			if in_callable {
				let mut nc = spec.walk();
				for n in spec.named_children(&mut nc) {
					if n.kind() == "identifier"
						&& let Ok(s) = n.utf8_text(self.source_bytes)
						&& s != "_"
					{
						self.record_local(s.as_bytes());
						if self.deep {
							let m = extend_segment(scope, kinds::LOCAL, s.as_bytes());
							let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(n)));
						}
					}
				}
			}
			if let Some(value) = spec.child_by_field_name("value") {
				self.dispatch(value, scope, graph);
			}
		}
	}

	fn handle_range_clause(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if is_callable_scope(scope, &self.module)
			&& let Some(left) = node.child_by_field_name("left")
		{
			self.bind_locals_in_expression_list(left, scope, graph);
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.dispatch(right, scope, graph);
		}
	}

	fn bind_locals_in_expression_list(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		match node.kind() {
			"identifier" => {
				let Ok(s) = node.utf8_text(self.source_bytes) else {
					return;
				};
				if s.is_empty() || s == "_" {
					return;
				}
				self.record_local(s.as_bytes());
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, s.as_bytes());
					let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(node)));
				}
			}
			"expression_list" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					self.bind_locals_in_expression_list(c, scope, graph);
				}
			}
			_ => {}
		}
	}

	fn bind_param_identifiers(
		&self,
		container: Node<'_>,
		callable_m: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = container.walk();
		for child in container.named_children(&mut cursor) {
			if !matches!(
				child.kind(),
				"parameter_declaration" | "variadic_parameter_declaration"
			) {
				continue;
			}
			let mut nc = child.walk();
			for n in child.named_children(&mut nc) {
				if n.kind() == "identifier"
					&& let Ok(s) = n.utf8_text(self.source_bytes)
					&& s != "_"
				{
					self.record_local(s.as_bytes());
					if self.deep {
						let m = extend_segment(callable_m, kinds::PARAM, s.as_bytes());
						let _ = graph.add_def(m, kinds::PARAM, callable_m, Some(node_position(n)));
					}
				}
			}
		}
	}

	fn handle_function(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let types = function_param_types(node, self.source_bytes);
		let signature = crate::lang::callable::join_bytes_with_comma(&types);
		let m = extend_callable_typed(scope, kinds::FUNC, name.as_bytes(), &types);
		let attrs = DefAttrs {
			visibility: visibility_from_name(name.as_bytes()),
			signature: &signature,
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::FUNC,
			scope,
			Some(node_position(node)),
			&attrs,
		);
		self.push_local_scope();
		self.handle_callable_params(node, &m, graph);
		self.emit_callable_type_refs(node, &m, graph);
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
		self.pop_local_scope();
	}

	pub(super) fn field_text(&self, node: Node<'_>, field: &str) -> Option<&'src str> {
		node.child_by_field_name(field)?
			.utf8_text(self.source_bytes)
			.ok()
	}

	#[allow(dead_code)]
	pub(super) fn text_of(&self, node: Node<'_>) -> &'src str {
		node.utf8_text(self.source_bytes).unwrap_or("")
	}
}

pub(super) fn collect_type_table<'src>(
	root: Node<'_>,
	source: &'src [u8],
	parent: &Moniker,
	graph: &mut CodeGraph,
	out: &mut HashMap<&'src [u8], Moniker>,
) {
	let mut cursor = root.walk();
	for decl in root.children(&mut cursor) {
		if decl.kind() != "type_declaration" {
			continue;
		}
		let mut tc = decl.walk();
		for tspec in decl.named_children(&mut tc) {
			let (kind, name_node) = match tspec.kind() {
				"type_spec" => {
					let kind = match tspec.child_by_field_name("type").map(|n| n.kind()) {
						Some("struct_type") => kinds::STRUCT,
						Some("interface_type") => kinds::INTERFACE,
						_ => kinds::TYPE,
					};
					(kind, tspec.child_by_field_name("name"))
				}
				"type_alias" => (kinds::TYPE, tspec.child_by_field_name("name")),
				_ => continue,
			};
			let Some(name_node) = name_node else { continue };
			let Ok(name) = name_node.utf8_text(source) else {
				continue;
			};
			let m = extend_segment(parent, kind, name.as_bytes());
			let attrs = DefAttrs {
				visibility: super::scope::visibility_from_name(name.as_bytes()),
				..DefAttrs::default()
			};
			let _ =
				graph.add_def_attrs(m.clone(), kind, parent, Some(node_position(tspec)), &attrs);
			out.entry(name.as_bytes()).or_insert(m);
		}
	}
}

fn receiver_type_name<'a>(receiver: Node<'_>, source: &'a [u8]) -> Option<&'a [u8]> {
	let mut cursor = receiver.walk();
	let param = receiver.named_children(&mut cursor).next()?;
	if param.kind() != "parameter_declaration" {
		return None;
	}
	let type_node = param.child_by_field_name("type")?;
	extract_type_name(type_node, source)
}

fn extract_type_name<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a [u8]> {
	match node.kind() {
		"type_identifier" => node.utf8_text(source).ok().map(|s| s.as_bytes()),
		"pointer_type" => {
			let mut cursor = node.walk();
			let inner = node.named_children(&mut cursor).next()?;
			extract_type_name(inner, source)
		}
		"generic_type" => {
			let inner = node.child_by_field_name("type")?;
			extract_type_name(inner, source)
		}
		_ => None,
	}
}
