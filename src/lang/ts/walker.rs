use std::cell::RefCell;
use std::collections::HashSet;

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs};
use crate::core::moniker::Moniker;

use super::canonicalize::{
	anonymous_callback_name, callable_param_types, extend_callable_typed, extend_segment,
	extend_segment_u32, node_position,
};
use super::kinds;
use super::scope::{class_member_visibility, collect_binding_names, is_callable_scope};

pub(super) struct Callable<'tree, 'vis> {
	pub callable_node: Node<'tree>,
	pub anchor_node: Node<'tree>,
	pub name: &'tree [u8],
	pub kind: &'tree [u8],
	pub visibility: &'vis [u8],
}

pub(super) struct Walker<'src> {
	pub(super) source_bytes: &'src [u8],
	pub(super) module: Moniker,
	pub(super) deep: bool,
	pub(super) presets: &'src super::Presets,
	pub(super) export_ranges: Vec<(u32, u32)>,
	pub(super) local_scope: RefCell<Vec<HashSet<&'src [u8]>>>,
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
			"comment" => self.handle_comment(node, scope, graph),
			"import_statement" => self.handle_import(node, scope, graph),
			"export_statement" => self.handle_export(node, scope, graph),
			"class_declaration" | "abstract_class_declaration" => {
				self.handle_class(node, scope, graph)
			}
			"interface_declaration" => self.handle_interface(node, scope, graph),
			"enum_declaration" => self.handle_enum(node, scope, graph),
			"type_alias_declaration" => self.handle_type_alias(node, scope, graph),
			"function_declaration" | "generator_function_declaration" => {
				self.handle_function_decl(node, scope, graph)
			}
			"lexical_declaration" | "variable_declaration" => {
				self.handle_lexical(node, scope, graph)
			}
			"call_expression" => self.handle_call(node, scope, graph),
			"new_expression" => self.handle_new(node, scope, graph),
			"decorator" => self.handle_decorator(node, scope, graph),
			"type_annotation"
			| "type_arguments"
			| "union_type"
			| "intersection_type"
			| "lookup_type"
			| "index_type_query"
			| "type_query"
			| "generic_type"
			| "nested_type_identifier" => {
				self.emit_uses_type_recursive(node, scope, graph);
			}
			"return_statement"
			| "spread_element"
			| "parenthesized_expression"
			| "template_substitution"
			| "arguments"
			| "array" => {
				self.emit_reads_in_children(node, scope, graph);
			}
			"binary_expression" | "assignment_expression" => {
				self.handle_binary_like(node, scope, graph);
			}
			"unary_expression" | "update_expression" => {
				self.handle_unary_like(node, scope, graph);
			}
			"ternary_expression" => self.handle_ternary(node, scope, graph),
			"member_expression" | "subscript_expression" => {
				self.handle_member_like(node, scope, graph);
			}
			"shorthand_property_identifier" => self.emit_read_at(node, scope, graph),
			"jsx_expression" => self.emit_reads_in_children(node, scope, graph),
			"jsx_opening_element" | "jsx_self_closing_element" => {
				self.handle_jsx_element(node, scope, graph)
			}
			"pair" => self.handle_pair(node, scope, graph),
			"arrow_function" | "function_expression" => {
				self.handle_inline_callable(node, scope, graph)
			}
			"catch_clause" => self.handle_catch_clause(node, scope, graph),
			"for_in_statement" | "for_of_statement" => self.handle_for_in(node, scope, graph),
			_ => self.walk(node, scope, graph),
		}
	}

	fn handle_comment(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let m = extend_segment_u32(scope, kinds::COMMENT, node.start_byte() as u32);
		let _ = graph.add_def(m, kinds::COMMENT, scope, Some(node_position(node)));
	}

	fn handle_export(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if node.child_by_field_name("source").is_some() {
			self.handle_reexport(node, scope, graph);
			return;
		}
		let mut cursor = node.walk();
		let mut has_default = false;
		for c in node.children(&mut cursor) {
			if c.kind() == "default" {
				has_default = true;
				break;
			}
		}
		if has_default {
			let public = DefAttrs {
				visibility: kinds::VIS_PUBLIC,
				..DefAttrs::default()
			};
			let mut cursor = node.walk();
			for c in node.children(&mut cursor) {
				match c.kind() {
					"function_expression" | "arrow_function" => {
						let types = callable_param_types(c, self.source_bytes);
						let m = extend_callable_typed(scope, kinds::FUNCTION, b"default", &types);
						let _ = graph.add_def_attrs(
							m.clone(),
							kinds::FUNCTION,
							scope,
							Some(node_position(c)),
							&public,
						);
						if let Some(body) = c.child_by_field_name("body") {
							self.walk(body, &m, graph);
						}
						return;
					}
					"class" | "class_declaration" => {
						let m = extend_segment(scope, kinds::CLASS, b"default");
						let _ = graph.add_def_attrs(
							m.clone(),
							kinds::CLASS,
							scope,
							Some(node_position(c)),
							&public,
						);
						if let Some(body) = c.child_by_field_name("body") {
							self.walk_class_body(body, &m, graph);
						}
						return;
					}
					_ => {}
				}
			}
		}
		self.walk(node, scope, graph);
	}

	fn handle_class(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(scope, kinds::CLASS, name.as_bytes());
		let attrs = DefAttrs {
			visibility: self.module_visibility(node),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::CLASS,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"class_heritage" => self.handle_class_heritage(child, &m, graph),
				"decorator" => self.handle_decorator(child, &m, graph),
				"class_body" => self.walk_class_body(child, &m, graph),
				_ => {}
			}
		}
	}

	fn walk_class_body(&self, body: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = body.walk();
		for child in body.children(&mut cursor) {
			match child.kind() {
				"method_definition" | "method_signature" => {
					self.handle_method(child, parent, graph)
				}
				"public_field_definition" | "property_signature" => {
					self.handle_field(child, parent, graph)
				}
				"decorator" => self.handle_decorator(child, parent, graph),
				"comment" => self.handle_comment(child, parent, graph),
				_ => {}
			}
		}
	}

	fn handle_method(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let types = callable_param_types(node, self.source_bytes);
		let is_ctor = name == "constructor";
		let kind: &[u8] = if is_ctor {
			kinds::CONSTRUCTOR
		} else {
			kinds::METHOD
		};
		let m = extend_callable_typed(parent, kind, name.as_bytes(), &types);
		let attrs = DefAttrs {
			visibility: class_member_visibility(node, self.source_bytes),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(m.clone(), kind, parent, Some(node_position(node)), &attrs);

		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "decorator" {
				self.handle_decorator(c, &m, graph);
			}
		}

		if let Some(rt) = node.child_by_field_name("return_type") {
			self.emit_uses_type_recursive(rt, &m, graph);
		}

		if let Some(params) = node.child_by_field_name("parameters") {
			self.handle_parameters(params, &m, graph);
		}

		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
	}

	fn handle_field(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(parent, kinds::FIELD, name.as_bytes());
		let attrs = DefAttrs {
			visibility: class_member_visibility(node, self.source_bytes),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::FIELD,
			parent,
			Some(node_position(node)),
			&attrs,
		);

		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "decorator" {
				self.handle_decorator(c, &m, graph);
			}
		}

		if let Some(tp) = node.child_by_field_name("type") {
			self.emit_uses_type_recursive(tp, &m, graph);
		}
		if let Some(value) = node.child_by_field_name("value") {
			self.dispatch(value, &m, graph);
		}
	}

	fn handle_interface(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(scope, kinds::INTERFACE, name.as_bytes());
		let attrs = DefAttrs {
			visibility: self.module_visibility(node),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::INTERFACE,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"extends_type_clause" | "extends_clause" => {
					self.emit_heritage_refs(child, &m, kinds::EXTENDS, graph);
				}
				"interface_body" | "object_type" => {
					let mut bc = child.walk();
					for member in child.children(&mut bc) {
						match member.kind() {
							"property_signature" => self.handle_field(member, &m, graph),
							"method_signature" | "method_definition" => {
								self.handle_method(member, &m, graph)
							}
							_ => {}
						}
					}
				}
				_ => {}
			}
		}
	}

	fn handle_enum(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(scope, kinds::ENUM, name.as_bytes());
		let attrs = DefAttrs {
			visibility: self.module_visibility(node),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::ENUM,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		if let Some(body) = node.child_by_field_name("body") {
			let mut cursor = body.walk();
			for member in body.named_children(&mut cursor) {
				if member.kind() == "enum_assignment" || member.kind() == "property_identifier" {
					let name_node = if member.kind() == "enum_assignment" {
						member.child_by_field_name("name").unwrap_or(member)
					} else {
						member
					};
					if let Ok(member_name) = name_node.utf8_text(self.source_bytes) {
						let mm = extend_segment(&m, kinds::ENUM_CONSTANT, member_name.as_bytes());
						let _ = graph.add_def(
							mm,
							kinds::ENUM_CONSTANT,
							&m,
							Some(node_position(member)),
						);
					}
				}
			}
		}
	}

	fn handle_type_alias(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(scope, kinds::TYPE, name.as_bytes());
		let attrs = DefAttrs {
			visibility: self.module_visibility(node),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::TYPE,
			scope,
			Some(node_position(node)),
			&attrs,
		);
		if let Some(value) = node.child_by_field_name("value") {
			self.emit_uses_type_recursive(value, &m, graph);
		}
	}

	fn handle_function_decl(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let vis = self.module_visibility(node);
		self.emit_callable(
			Callable {
				callable_node: node,
				anchor_node: node,
				name: name.as_bytes(),
				kind: kinds::FUNCTION,
				visibility: vis,
			},
			scope,
			graph,
		);
	}

	fn handle_lexical(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let inside_callable = is_callable_scope(scope, &self.module);
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			self.handle_variable_declarator(child, scope, inside_callable, graph);
		}
	}

	fn handle_variable_declarator(
		&self,
		decl: Node<'_>,
		scope: &Moniker,
		inside_callable: bool,
		graph: &mut CodeGraph,
	) {
		let Some(name_node) = decl.child_by_field_name("name") else {
			return;
		};
		let value = decl.child_by_field_name("value");
		let type_annot = decl.child_by_field_name("type");

		let names = collect_binding_names(name_node, self.source_bytes);

		let module_vis = self.module_visibility(decl);
		for name in &names {
			if inside_callable {
				self.record_local(name.as_bytes());
			}
			let (kind, emit) = if inside_callable {
				(kinds::LOCAL, self.deep)
			} else if let Some(v) =
				value.filter(|v| v.kind() == "arrow_function" || v.kind() == "function_expression")
			{
				self.emit_callable(
					Callable {
						callable_node: v,
						anchor_node: decl,
						name: name.as_bytes(),
						kind: kinds::FUNCTION,
						visibility: module_vis,
					},
					scope,
					graph,
				);
				continue;
			} else {
				(kinds::CONST, true)
			};
			if emit {
				let m = extend_segment(scope, kind, name.as_bytes());
				let attrs = DefAttrs {
					visibility: if inside_callable {
						kinds::VIS_NONE
					} else {
						module_vis
					},
					..DefAttrs::default()
				};
				let _ = graph.add_def_attrs(m, kind, scope, Some(node_position(decl)), &attrs);
			}
		}

		if let Some(tp) = type_annot {
			self.emit_uses_type_recursive(tp, scope, graph);
		}
		if let Some(v) = value {
			self.dispatch(v, scope, graph);
		}
	}

	fn handle_parameters(&self, params: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"required_parameter" | "optional_parameter" => {
					let pat = child.child_by_field_name("pattern");
					let tp = child.child_by_field_name("type");
					if let Some(p) = pat {
						self.emit_param_leaf(p, callable, graph);
					}
					if let Some(t) = tp {
						self.emit_uses_type_recursive(t, callable, graph);
					}
					let mut cc = child.walk();
					for c in child.children(&mut cc) {
						if c.kind() == "decorator" {
							self.handle_decorator(c, callable, graph);
						}
					}
				}
				"rest_pattern" => {
					self.emit_param_leaf(child, callable, graph);
				}
				_ => {}
			}
		}
	}

	fn emit_param_leaf(&self, pat: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		for name in collect_binding_names(pat, self.source_bytes) {
			self.record_local(name.as_bytes());
			if self.deep {
				let m = extend_segment(callable, kinds::PARAM, name.as_bytes());
				let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(pat)));
			}
		}
	}

	fn handle_inline_callable(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if self.deep && is_callable_scope(scope, &self.module) {
			let name = anonymous_callback_name(node);
			self.emit_callable(
				Callable {
					callable_node: node,
					anchor_node: node,
					name: &name,
					kind: kinds::FUNCTION,
					visibility: kinds::VIS_NONE,
				},
				scope,
				graph,
			);
			return;
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			self.walk(params, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, scope, graph);
		}
	}

	fn handle_catch_clause(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if is_callable_scope(scope, &self.module)
			&& let Some(p) = node.child_by_field_name("parameter")
		{
			self.emit_param_leaf(p, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, scope, graph);
		}
	}

	fn handle_for_in(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if is_callable_scope(scope, &self.module) {
			let mut cursor = node.walk();
			for c in node.named_children(&mut cursor) {
				if c.kind() == "identifier" {
					let name = self.text_of(c);
					self.record_local(name.as_bytes());
					if self.deep {
						let m = extend_segment(scope, kinds::LOCAL, name.as_bytes());
						let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(c)));
					}
					break;
				}
			}
		}
		self.walk(node, scope, graph);
	}

	fn handle_pair(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if self.deep && is_callable_scope(scope, &self.module) {
			let key = node.child_by_field_name("key");
			let value = node.child_by_field_name("value");
			if let (Some(k), Some(v)) = (key, value)
				&& k.kind() == "property_identifier"
				&& (v.kind() == "arrow_function" || v.kind() == "function_expression")
			{
				let name = self.text_of(k);
				self.emit_callable(
					Callable {
						callable_node: v,
						anchor_node: node,
						name: name.as_bytes(),
						kind: kinds::FUNCTION,
						visibility: kinds::VIS_PUBLIC,
					},
					scope,
					graph,
				);
				return;
			}
		}
		self.walk(node, scope, graph);
	}

	fn emit_callable(
		&self,
		c: Callable<'_, '_>,
		parent: &Moniker,
		graph: &mut CodeGraph,
	) -> Moniker {
		let types = callable_param_types(c.callable_node, self.source_bytes);
		let m = extend_callable_typed(parent, c.kind, c.name, &types);
		let attrs = DefAttrs {
			visibility: c.visibility,
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			c.kind,
			parent,
			Some(node_position(c.anchor_node)),
			&attrs,
		);
		self.push_local_scope();
		if let Some(rt) = c.callable_node.child_by_field_name("return_type") {
			self.emit_uses_type_recursive(rt, &m, graph);
		}
		if let Some(params) = c.callable_node.child_by_field_name("parameters") {
			self.handle_parameters(params, &m, graph);
		}
		if let Some(p) = c.callable_node.child_by_field_name("parameter") {
			self.emit_param_leaf(p, &m, graph);
		}
		if let Some(body) = c.callable_node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
		self.pop_local_scope();
		m
	}

	pub(super) fn field_text(&self, node: Node<'_>, field: &str) -> Option<&'src str> {
		node.child_by_field_name(field)?
			.utf8_text(self.source_bytes)
			.ok()
	}

	pub(super) fn text_of(&self, node: Node<'_>) -> &'src str {
		node.utf8_text(self.source_bytes).unwrap_or("")
	}
}
