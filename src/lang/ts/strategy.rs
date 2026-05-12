use std::cell::RefCell;
use std::collections::HashSet;

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{extend_callable_arity, extend_segment};
use crate::lang::strategy::{LangStrategy, NodeShape, RefSpec, Symbol};
use crate::lang::tree_util::{node_position, node_slice};

use super::canonicalize::{
	anonymous_callback_name, append_module_segments, callable_param_types, extend_callable_typed,
	external_pkg_builder, strip_known_extension,
};
use super::kinds;

pub(super) struct Strategy<'src> {
	pub(super) module: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) presets: &'src super::Presets,
	pub(super) export_ranges: Vec<(u32, u32)>,
	pub(super) local_scope: RefCell<Vec<HashSet<Vec<u8>>>>,
}

impl<'a> LangStrategy for Strategy<'a> {
	fn classify<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
	) -> NodeShape<'src> {
		match node.kind() {
			"comment" => NodeShape::Annotation {
				kind: kinds::COMMENT,
			},
			"import_statement" => {
				self.handle_import(node, scope, graph);
				NodeShape::Skip
			}
			"export_statement" => self.classify_export(node, scope, source, graph),
			"class_declaration" | "abstract_class_declaration" => {
				self.classify_class(node, scope, source, None, None)
			}
			"interface_declaration" => self.classify_interface(node, scope, source),
			"enum_declaration" => self.classify_enum(node, scope, source),
			"type_alias_declaration" => self.classify_type_alias(node, scope, source, graph),
			"function_declaration" | "generator_function_declaration" => {
				self.classify_function_decl(node, scope, source)
			}
			"method_definition" | "method_signature" => {
				self.classify_method(node, scope, source, graph)
			}
			"public_field_definition" | "property_signature" => {
				self.classify_field(node, scope, source, graph)
			}
			"lexical_declaration" | "variable_declaration" => {
				self.handle_lexical(node, scope, graph);
				NodeShape::Skip
			}
			"call_expression" => {
				self.handle_call(node, scope, graph);
				NodeShape::Skip
			}
			"new_expression" => {
				self.handle_new(node, scope, graph);
				NodeShape::Skip
			}
			"decorator" => {
				self.handle_decorator(node, scope, graph);
				NodeShape::Skip
			}
			"arrow_function" | "function_expression" => {
				self.classify_inline_callable(node, scope, source)
			}
			"pair" => self.classify_pair(node, scope, source),
			"catch_clause" => {
				self.handle_catch_clause(node, scope, graph);
				NodeShape::Skip
			}
			"for_in_statement" | "for_of_statement" => {
				self.handle_for_in(node, scope, graph);
				NodeShape::Skip
			}
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
				NodeShape::Skip
			}
			"return_statement"
			| "spread_element"
			| "parenthesized_expression"
			| "template_substitution"
			| "arguments"
			| "array" => {
				self.emit_reads_in_children(node, scope, graph);
				NodeShape::Skip
			}
			"binary_expression" | "assignment_expression" => {
				self.handle_binary_like(node, scope, graph);
				NodeShape::Skip
			}
			"unary_expression" | "update_expression" => {
				self.handle_unary_like(node, scope, graph);
				NodeShape::Skip
			}
			"ternary_expression" => {
				self.handle_ternary(node, scope, graph);
				NodeShape::Skip
			}
			"member_expression" | "subscript_expression" => {
				self.handle_member_like(node, scope, graph);
				NodeShape::Skip
			}
			"shorthand_property_identifier" => {
				self.emit_read_at(node, scope, graph);
				NodeShape::Skip
			}
			"jsx_expression" => {
				self.emit_reads_in_children(node, scope, graph);
				NodeShape::Skip
			}
			"jsx_opening_element" | "jsx_self_closing_element" => {
				self.handle_jsx_element(node, scope, graph);
				NodeShape::Skip
			}
			_ => NodeShape::Recurse,
		}
	}

	fn before_body(
		&self,
		node: Node<'_>,
		kind: &[u8],
		moniker: &Moniker,
		_source: &[u8],
		graph: &mut CodeGraph,
	) {
		if !is_callable_kind(kind) {
			return;
		}
		if let Some(rt) = node.child_by_field_name("return_type") {
			self.emit_uses_type_recursive(rt, moniker, graph);
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			self.emit_param_defs_and_types(params, moniker, graph);
		}
		if let Some(p) = node.child_by_field_name("parameter") {
			self.emit_param_leaf(p, moniker, graph);
		}
	}

	fn after_body(&self, kind: &[u8], _moniker: &Moniker) {
		if is_callable_kind(kind) {
			self.pop_local_scope();
		}
	}

	fn on_symbol_emitted(
		&self,
		node: Node<'_>,
		sym_kind: &[u8],
		sym_moniker: &Moniker,
		_source: &[u8],
		graph: &mut CodeGraph,
	) {
		if sym_kind == kinds::CLASS
			|| sym_kind == kinds::INTERFACE
			|| sym_kind == kinds::FUNCTION
			|| sym_kind == kinds::METHOD
			|| sym_kind == kinds::CONSTRUCTOR
			|| sym_kind == kinds::FIELD
		{
			let mut cursor = node.walk();
			for c in node.children(&mut cursor) {
				if c.kind() == "decorator" {
					self.walk_decorator_args(c, sym_moniker, graph);
				}
			}
		}
		if sym_kind == kinds::ENUM {
			self.emit_enum_constants(node, sym_moniker, graph);
		}
		if sym_kind == kinds::FIELD {
			if let Some(tp) = node.child_by_field_name("type") {
				self.emit_uses_type_recursive(tp, sym_moniker, graph);
			}
			if let Some(value) = node.child_by_field_name("value") {
				self.recurse_subtree(value, sym_moniker, graph);
			}
		}
	}
}

impl<'src_lang> Strategy<'src_lang> {
	fn classify_class<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		name_override: Option<&'static [u8]>,
		visibility_override: Option<&'static [u8]>,
	) -> NodeShape<'src> {
		let name: &[u8] = if let Some(n) = name_override {
			n
		} else {
			let Some(name_node) = node.child_by_field_name("name") else {
				return NodeShape::Recurse;
			};
			node_slice(name_node, source)
		};
		let moniker = extend_segment(scope, kinds::CLASS, name);

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		self.collect_decorator_refs(node, &mut annotated_by);
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() == "class_heritage" {
				self.collect_heritage_refs_from_clauses(child, &mut annotated_by);
			}
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::CLASS,
			visibility: visibility_override.unwrap_or_else(|| self.module_visibility(node)),
			signature: None,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_interface<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::INTERFACE, name);

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if matches!(child.kind(), "extends_type_clause" | "extends_clause") {
				self.emit_heritage_refs_collect(child, kinds::EXTENDS, &mut annotated_by);
			}
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::INTERFACE,
			visibility: self.module_visibility(node),
			signature: None,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_enum<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::ENUM, name);
		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::ENUM,
			visibility: self.module_visibility(node),
			signature: None,
			body: None,
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn classify_type_alias<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::TYPE, name);
		if let Some(value) = node.child_by_field_name("value") {
			self.emit_uses_type_recursive(value, &moniker, graph);
		}
		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::TYPE,
			visibility: self.module_visibility(node),
			signature: None,
			body: None,
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn classify_function_decl<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		self.callable_symbol(
			node,
			node,
			name,
			kinds::FUNCTION,
			scope,
			self.module_visibility(node),
		)
	}

	fn classify_method<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let kind: &'static [u8] = if name == b"constructor" {
			kinds::CONSTRUCTOR
		} else {
			kinds::METHOD
		};
		let vis = class_member_visibility(node, source);
		let _ = graph;
		self.callable_symbol(node, node, name, kind, scope, vis)
	}

	fn classify_field<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		_graph: &mut CodeGraph,
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::FIELD, name);

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::FIELD,
			visibility: class_member_visibility(node, source),
			signature: None,
			body: None,
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn classify_inline_callable<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		if self.deep && is_callable_scope(scope, &self.module) {
			let name = anonymous_callback_name(node);
			return self.callable_symbol(
				node,
				node,
				&name,
				kinds::FUNCTION,
				scope,
				kinds::VIS_NONE,
			);
		}
		let _ = source;
		NodeShape::Recurse
	}

	fn classify_pair<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		if self.deep && is_callable_scope(scope, &self.module) {
			let key = node.child_by_field_name("key");
			let value = node.child_by_field_name("value");
			if let (Some(k), Some(v)) = (key, value)
				&& k.kind() == "property_identifier"
				&& (v.kind() == "arrow_function" || v.kind() == "function_expression")
			{
				let name = node_slice(k, source);
				return self.callable_symbol(
					v,
					node,
					name,
					kinds::FUNCTION,
					scope,
					kinds::VIS_PUBLIC,
				);
			}
		}
		NodeShape::Recurse
	}

	fn classify_export<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
	) -> NodeShape<'src> {
		if node.child_by_field_name("source").is_some() {
			self.handle_reexport(node, scope, graph);
			return NodeShape::Skip;
		}
		let mut has_default = false;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "default" {
				has_default = true;
				break;
			}
		}
		if has_default {
			let mut cursor = node.walk();
			for c in node.children(&mut cursor) {
				match c.kind() {
					"function_expression" | "arrow_function" => {
						return self.callable_symbol(
							c,
							c,
							b"default",
							kinds::FUNCTION,
							scope,
							kinds::VIS_PUBLIC,
						);
					}
					"class" | "class_declaration" => {
						return self.classify_class(
							c,
							scope,
							source,
							Some(b"default"),
							Some(kinds::VIS_PUBLIC),
						);
					}
					_ => {}
				}
			}
		}
		NodeShape::Recurse
	}

	fn callable_symbol<'src>(
		&self,
		callable_node: Node<'src>,
		anchor_node: Node<'src>,
		name: &[u8],
		kind: &'static [u8],
		scope: &Moniker,
		visibility: &'static [u8],
	) -> NodeShape<'src> {
		let types = callable_param_types(callable_node, self.source_bytes);
		let moniker = extend_callable_typed(scope, kind, name, &types);

		self.push_local_scope();
		if let Some(params) = callable_node.child_by_field_name("parameters") {
			self.record_param_locals(params);
		}
		if let Some(p) = callable_node.child_by_field_name("parameter") {
			self.record_pat_locals(p);
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility,
			signature: None,
			body: callable_node.child_by_field_name("body"),
			position: node_position(anchor_node),
			annotated_by: Vec::new(),
		})
	}

	fn emit_enum_constants(&self, enum_node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(body) = enum_node.child_by_field_name("body") else {
			return;
		};
		let mut cursor = body.walk();
		for member in body.named_children(&mut cursor) {
			if member.kind() != "enum_assignment" && member.kind() != "property_identifier" {
				continue;
			}
			let name_node = if member.kind() == "enum_assignment" {
				member.child_by_field_name("name").unwrap_or(member)
			} else {
				member
			};
			let name = node_slice(name_node, self.source_bytes);
			if name.is_empty() {
				continue;
			}
			let m = extend_segment(parent, kinds::ENUM_CONSTANT, name);
			let _ = graph.add_def(m, kinds::ENUM_CONSTANT, parent, Some(node_position(member)));
		}
	}

	fn record_param_locals(&self, params: Node<'_>) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"required_parameter" | "optional_parameter" => {
					if let Some(p) = child.child_by_field_name("pattern") {
						for n in collect_binding_names(p, self.source_bytes) {
							self.record_local(&n);
						}
					}
				}
				"rest_pattern" => {
					for n in collect_binding_names(child, self.source_bytes) {
						self.record_local(&n);
					}
				}
				_ => {}
			}
		}
	}

	fn record_pat_locals(&self, pat: Node<'_>) {
		for n in collect_binding_names(pat, self.source_bytes) {
			self.record_local(&n);
		}
	}

	fn emit_param_defs_and_types(
		&self,
		params: Node<'_>,
		callable: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"required_parameter" | "optional_parameter" => {
					if let Some(p) = child.child_by_field_name("pattern") {
						self.emit_param_leaf(p, callable, graph);
					}
					if let Some(t) = child.child_by_field_name("type") {
						self.emit_uses_type_recursive(t, callable, graph);
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
			if self.deep {
				let m = extend_segment(callable, kinds::PARAM, &name);
				let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(pat)));
			}
		}
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
				self.record_local(name);
			}
			let (kind, emit) = if inside_callable {
				(kinds::LOCAL, self.deep)
			} else if let Some(v) =
				value.filter(|v| v.kind() == "arrow_function" || v.kind() == "function_expression")
			{
				let visibility = module_vis;
				let types = callable_param_types(v, self.source_bytes);
				let m = extend_callable_typed(scope, kinds::FUNCTION, name, &types);
				let attrs = crate::core::code_graph::DefAttrs {
					visibility,
					..crate::core::code_graph::DefAttrs::default()
				};
				let _ = graph.add_def_attrs(
					m.clone(),
					kinds::FUNCTION,
					scope,
					Some(node_position(decl)),
					&attrs,
				);
				self.push_local_scope();
				if let Some(rt) = v.child_by_field_name("return_type") {
					self.emit_uses_type_recursive(rt, &m, graph);
				}
				if let Some(params) = v.child_by_field_name("parameters") {
					self.record_param_locals(params);
					self.emit_param_defs_and_types(params, &m, graph);
				}
				if let Some(p) = v.child_by_field_name("parameter") {
					self.record_pat_locals(p);
					self.emit_param_leaf(p, &m, graph);
				}
				if let Some(body) = v.child_by_field_name("body") {
					self.walk_children(body, &m, graph);
				}
				self.pop_local_scope();
				continue;
			} else {
				(kinds::CONST, true)
			};
			if emit {
				let m = extend_segment(scope, kind, name);
				let attrs = crate::core::code_graph::DefAttrs {
					visibility: if inside_callable {
						kinds::VIS_NONE
					} else {
						module_vis
					},
					..crate::core::code_graph::DefAttrs::default()
				};
				let _ = graph.add_def_attrs(m, kind, scope, Some(node_position(decl)), &attrs);
			}
		}

		if let Some(tp) = type_annot {
			self.emit_uses_type_recursive(tp, scope, graph);
		}
		if let Some(v) = value {
			self.recurse_subtree(v, scope, graph);
		}
	}

	fn handle_catch_clause(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if is_callable_scope(scope, &self.module)
			&& let Some(p) = node.child_by_field_name("parameter")
		{
			for n in collect_binding_names(p, self.source_bytes) {
				self.record_local(&n);
			}
			self.emit_param_leaf(p, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_children(body, scope, graph);
		}
	}

	fn handle_for_in(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if is_callable_scope(scope, &self.module) {
			let mut cursor = node.walk();
			for c in node.named_children(&mut cursor) {
				if c.kind() == "identifier" {
					let name = node_slice(c, self.source_bytes);
					self.record_local(name);
					if self.deep {
						let m = extend_segment(scope, kinds::LOCAL, name);
						let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(c)));
					}
					break;
				}
			}
		}
		self.walk_children(node, scope, graph);
	}

	fn handle_call(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let arity = call_argument_count(node);
		let Some(fn_node) = node.child_by_field_name("function") else {
			self.walk_children(node, scope, graph);
			return;
		};
		match fn_node.kind() {
			"identifier" => {
				let name = node_slice(fn_node, self.source_bytes);
				match self.name_confidence(name) {
					Some(confidence) => {
						let target = if confidence == kinds::CONF_LOCAL {
							extend_segment(scope, kinds::LOCAL, name)
						} else {
							extend_callable_arity(&self.module, kinds::FUNCTION, name, arity)
						};
						let attrs = RefAttrs {
							confidence,
							..RefAttrs::default()
						};
						let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
						self.maybe_emit_di_register(node, name, scope, graph, pos);
					}
					None => {
						self.maybe_emit_di_register(node, name, scope, graph, pos);
					}
				}
			}
			"member_expression" => {
				if let Some(prop) = fn_node.child_by_field_name("property") {
					let name = node_slice(prop, self.source_bytes);
					if !name.is_empty() {
						let target =
							extend_callable_arity(&self.module, kinds::METHOD, name, arity);
						let attrs = RefAttrs {
							receiver_hint: receiver_hint(fn_node, self.source_bytes),
							confidence: kinds::CONF_NAME_MATCH,
							..RefAttrs::default()
						};
						let _ = graph.add_ref_attrs(
							scope,
							target,
							kinds::METHOD_CALL,
							Some(pos),
							&attrs,
						);
						self.maybe_emit_di_register(node, name, scope, graph, pos);
					}
				}
				if let Some(obj) = fn_node.child_by_field_name("object") {
					self.recurse_subtree(obj, scope, graph);
				}
			}
			_ => {}
		}

		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk_children(args, scope, graph);
		}
	}

	fn handle_new(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		if let Some(ctor) = node.child_by_field_name("constructor") {
			let name = match ctor.kind() {
				"identifier" | "type_identifier" => Some(node_slice(ctor, self.source_bytes)),
				"member_expression" => ctor
					.child_by_field_name("property")
					.map(|p| node_slice(p, self.source_bytes)),
				_ => None,
			};
			if let Some(n) = name
				&& !n.is_empty()
			{
				let target = extend_segment(&self.module, kinds::CLASS, n);
				let attrs = RefAttrs {
					confidence: kinds::CONF_NAME_MATCH,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::INSTANTIATES, Some(pos), &attrs);
			}
		}
		self.walk_children(node, scope, graph);
	}

	fn handle_decorator(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"identifier" => {
					let name = node_slice(c, self.source_bytes);
					if !name.is_empty() {
						let target = extend_callable_arity(&self.module, kinds::FUNCTION, name, 0);
						let attrs = RefAttrs {
							confidence: kinds::CONF_NAME_MATCH,
							..RefAttrs::default()
						};
						let _ =
							graph.add_ref_attrs(scope, target, kinds::ANNOTATES, Some(pos), &attrs);
					}
				}
				"call_expression" => {
					if let Some(fn_node) = c.child_by_field_name("function")
						&& fn_node.kind() == "identifier"
					{
						let name = node_slice(fn_node, self.source_bytes);
						let arity = call_argument_count(c);
						let target =
							extend_callable_arity(&self.module, kinds::FUNCTION, name, arity);
						let attrs = RefAttrs {
							confidence: kinds::CONF_NAME_MATCH,
							..RefAttrs::default()
						};
						let _ =
							graph.add_ref_attrs(scope, target, kinds::ANNOTATES, Some(pos), &attrs);
					}
					if let Some(args) = c.child_by_field_name("arguments") {
						self.walk_children(args, scope, graph);
					}
				}
				_ => {}
			}
		}
	}

	fn walk_decorator_args(
		&self,
		decorator: Node<'_>,
		sym_moniker: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = decorator.walk();
		for c in decorator.children(&mut cursor) {
			if c.kind() == "call_expression"
				&& let Some(args) = c.child_by_field_name("arguments")
			{
				self.walk_children(args, sym_moniker, graph);
			}
		}
	}

	fn collect_decorator_refs(&self, node: Node<'_>, out: &mut Vec<RefSpec>) {
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() != "decorator" {
				continue;
			}
			let pos = node_position(c);
			let mut dc = c.walk();
			for ch in c.children(&mut dc) {
				match ch.kind() {
					"identifier" => {
						let name = node_slice(ch, self.source_bytes);
						if name.is_empty() {
							continue;
						}
						let target = extend_callable_arity(&self.module, kinds::FUNCTION, name, 0);
						out.push(RefSpec {
							kind: kinds::ANNOTATES,
							target,
							confidence: kinds::CONF_NAME_MATCH,
							position: pos,
							receiver_hint: b"",
							alias: b"",
						});
					}
					"call_expression" => {
						if let Some(fn_node) = ch.child_by_field_name("function")
							&& fn_node.kind() == "identifier"
						{
							let name = node_slice(fn_node, self.source_bytes);
							let arity = call_argument_count(ch);
							let target =
								extend_callable_arity(&self.module, kinds::FUNCTION, name, arity);
							out.push(RefSpec {
								kind: kinds::ANNOTATES,
								target,
								confidence: kinds::CONF_NAME_MATCH,
								position: pos,
								receiver_hint: b"",
								alias: b"",
							});
						}
					}
					_ => {}
				}
			}
		}
	}

	fn collect_heritage_refs_from_clauses(&self, node: Node<'_>, out: &mut Vec<RefSpec>) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			let edge: &'static [u8] = match child.kind() {
				"extends_clause" => kinds::EXTENDS,
				"implements_clause" => kinds::IMPLEMENTS,
				_ => continue,
			};
			self.emit_heritage_refs_collect(child, edge, out);
		}
	}

	fn emit_heritage_refs_collect(
		&self,
		clause: Node<'_>,
		edge: &'static [u8],
		out: &mut Vec<RefSpec>,
	) {
		let mut cursor = clause.walk();
		for c in clause.children(&mut cursor) {
			let pos = node_position(c);
			let target_kind = if edge == kinds::IMPLEMENTS {
				kinds::INTERFACE
			} else {
				kinds::CLASS
			};
			let name_opt: Option<Vec<u8>> = match c.kind() {
				"identifier" | "type_identifier" => Some(node_slice(c, self.source_bytes).to_vec()),
				"member_expression" => c
					.child_by_field_name("property")
					.map(|p| node_slice(p, self.source_bytes).to_vec()),
				"generic_type" => generic_short(c, self.source_bytes).map(|s| s.into_bytes()),
				"nested_type_identifier" => {
					nested_type_short(c, self.source_bytes).map(|s| s.into_bytes())
				}
				_ => None,
			};
			let Some(name) = name_opt else { continue };
			if name.is_empty() {
				continue;
			}
			let target = extend_segment(&self.module, target_kind, &name);
			out.push(RefSpec {
				kind: edge,
				target,
				confidence: kinds::CONF_NAME_MATCH,
				position: pos,
				receiver_hint: b"",
				alias: b"",
			});
		}
	}

	fn emit_uses_type_recursive(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		match node.kind() {
			"type_identifier" => {
				let name = node_slice(node, self.source_bytes);
				if name.is_empty() {
					return;
				}
				let target = extend_segment(&self.module, kinds::CLASS, name);
				let attrs = RefAttrs {
					confidence: kinds::CONF_NAME_MATCH,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					scope,
					target,
					kinds::USES_TYPE,
					Some(node_position(node)),
					&attrs,
				);
			}
			"nested_type_identifier" => {
				if let Some(name) = nested_type_short(node, self.source_bytes) {
					let target = extend_segment(&self.module, kinds::CLASS, name.as_bytes());
					let attrs = RefAttrs {
						confidence: kinds::CONF_NAME_MATCH,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::USES_TYPE,
						Some(node_position(node)),
						&attrs,
					);
				}
			}
			"generic_type" => {
				if let Some(name) = generic_short(node, self.source_bytes) {
					let target = extend_segment(&self.module, kinds::CLASS, name.as_bytes());
					let attrs = RefAttrs {
						confidence: kinds::CONF_NAME_MATCH,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::USES_TYPE,
						Some(node_position(node)),
						&attrs,
					);
				}
				if let Some(args) = node.child_by_field_name("type_arguments") {
					self.emit_uses_type_recursive(args, scope, graph);
				}
			}
			_ => {
				let mut cursor = node.walk();
				for c in node.children(&mut cursor) {
					self.emit_uses_type_recursive(c, scope, graph);
				}
			}
		}
	}

	fn emit_reads_in_children(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "identifier" {
				self.emit_read_at(c, scope, graph);
			} else {
				self.recurse_subtree(c, scope, graph);
			}
		}
	}

	fn emit_read_at(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let name = node_slice(node, self.source_bytes);
		if name.is_empty() {
			return;
		}
		let Some(confidence) = self.name_confidence(name) else {
			return;
		};
		let target = if confidence == kinds::CONF_LOCAL {
			extend_segment(scope, kinds::LOCAL, name)
		} else {
			extend_callable_typed(&self.module, kinds::FUNCTION, name, &[] as &[&[u8]])
		};
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::READS,
			Some(node_position(node)),
			&attrs,
		);
	}

	fn handle_member_like(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(obj) = node.child_by_field_name("object") {
			if obj.kind() == "identifier" {
				self.emit_read_at(obj, scope, graph);
			} else {
				self.recurse_subtree(obj, scope, graph);
			}
		}
	}

	fn handle_binary_like(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		for field in &["left", "right"] {
			if let Some(c) = node.child_by_field_name(field) {
				if c.kind() == "identifier" {
					self.emit_read_at(c, scope, graph);
				} else {
					self.recurse_subtree(c, scope, graph);
				}
			}
		}
	}

	fn handle_unary_like(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(arg) = node.child_by_field_name("argument") {
			if arg.kind() == "identifier" {
				self.emit_read_at(arg, scope, graph);
			} else {
				self.recurse_subtree(arg, scope, graph);
			}
		}
	}

	fn handle_ternary(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		for field in &["condition", "consequence", "alternative"] {
			if let Some(c) = node.child_by_field_name(field) {
				if c.kind() == "identifier" {
					self.emit_read_at(c, scope, graph);
				} else {
					self.recurse_subtree(c, scope, graph);
				}
			}
		}
	}

	fn handle_jsx_element(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(name) = node.child_by_field_name("name")
			&& name.kind() == "identifier"
		{
			self.emit_read_at(name, scope, graph);
		}
		self.walk_children(node, scope, graph);
	}

	fn handle_import(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(src_node) = node.child_by_field_name("source") else {
			return;
		};
		let raw_spec = unquote_string_literal(src_node, self.source_bytes);
		if raw_spec.is_empty() {
			return;
		}
		let pos = node_position(node);

		let mut clause: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "import_clause" {
				clause = Some(c);
				break;
			}
		}

		let confidence = import_confidence(raw_spec);
		let Some(clause) = clause else {
			let target = self.import_module_target(raw_spec);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
			return;
		};

		let mut cursor = clause.walk();
		for c in clause.children(&mut cursor) {
			match c.kind() {
				"identifier" => {
					let local_name = node_slice(c, self.source_bytes);
					let target = self.import_symbol_target(raw_spec, "default");
					let attrs = RefAttrs {
						alias: local_name,
						confidence,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::IMPORTS_SYMBOL,
						Some(pos),
						&attrs,
					);
				}
				"namespace_import" => {
					let alias = first_identifier_text(c, self.source_bytes);
					let target = self.import_module_target(raw_spec);
					let attrs = RefAttrs {
						alias,
						confidence,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::IMPORTS_MODULE,
						Some(pos),
						&attrs,
					);
				}
				"named_imports" => {
					let mut nc = c.walk();
					for spec in c.children(&mut nc) {
						if spec.kind() != "import_specifier" {
							continue;
						}
						let name = spec
							.child_by_field_name("name")
							.map(|n| {
								std::str::from_utf8(node_slice(n, self.source_bytes)).unwrap_or("")
							})
							.unwrap_or("");
						if name.is_empty() {
							continue;
						}
						let alias = spec
							.child_by_field_name("alias")
							.map(|n| node_slice(n, self.source_bytes))
							.unwrap_or(b"");
						let target = self.import_symbol_target(raw_spec, name);
						let attrs = RefAttrs {
							alias,
							confidence,
							..RefAttrs::default()
						};
						let _ = graph.add_ref_attrs(
							scope,
							target,
							kinds::IMPORTS_SYMBOL,
							Some(pos),
							&attrs,
						);
					}
				}
				_ => {}
			}
		}
	}

	fn handle_reexport(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(src_node) = node.child_by_field_name("source") else {
			return;
		};
		let raw_spec = unquote_string_literal(src_node, self.source_bytes);
		if raw_spec.is_empty() {
			return;
		}
		let pos = node_position(node);

		let mut has_star = false;
		let mut export_clause: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"*" => has_star = true,
				"export_clause" => export_clause = Some(c),
				_ => {}
			}
		}

		let confidence = import_confidence(raw_spec);
		if has_star {
			let target = self.import_module_target(raw_spec);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
			return;
		}

		let Some(clause) = export_clause else { return };
		let mut nc = clause.walk();
		for spec in clause.children(&mut nc) {
			if spec.kind() != "export_specifier" {
				continue;
			}
			let name = spec
				.child_by_field_name("name")
				.map(|n| std::str::from_utf8(node_slice(n, self.source_bytes)).unwrap_or(""))
				.unwrap_or("");
			if name.is_empty() {
				continue;
			}
			let alias = spec
				.child_by_field_name("alias")
				.map(|n| node_slice(n, self.source_bytes))
				.unwrap_or(b"");
			let target = self.import_symbol_target(raw_spec, name);
			let attrs = RefAttrs {
				alias,
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
		}
	}

	fn import_module_target(&self, raw_path: &str) -> Moniker {
		self.import_target(raw_path, None)
	}

	fn import_symbol_target(&self, raw_path: &str, name: &str) -> Moniker {
		self.import_target(raw_path, Some(name))
	}

	fn import_target(&self, raw_path: &str, symbol: Option<&str>) -> Moniker {
		let mut b = if is_relative_specifier(raw_path) {
			self.relative_module_builder(raw_path)
		} else {
			external_pkg_builder(self.module.as_view().project(), raw_path)
		};
		if let Some(sym) = symbol {
			b.segment(kinds::PATH, sym.as_bytes());
		}
		b.build()
	}

	fn relative_module_builder(&self, raw_path: &str) -> MonikerBuilder {
		let importer_view = self.module.as_view();
		let mut b = MonikerBuilder::from_view(importer_view);
		let mut depth = (importer_view.segment_count() as usize).saturating_sub(1);
		b.truncate(depth);

		let mut remainder = match raw_path {
			"." => "./",
			".." => "../",
			other => other,
		};
		while let Some(rest) = remainder.strip_prefix("./") {
			remainder = rest;
		}
		while let Some(rest) = remainder.strip_prefix("../") {
			depth = depth.saturating_sub(1);
			b.truncate(depth);
			remainder = rest;
		}
		let remainder = strip_known_extension(remainder);
		append_module_segments(&mut b, remainder);
		b
	}

	fn maybe_emit_di_register(
		&self,
		call: Node<'_>,
		callee_name: &[u8],
		scope: &Moniker,
		graph: &mut CodeGraph,
		pos: (u32, u32),
	) {
		if self.presets.di_register_callees.is_empty() {
			return;
		}
		let callee_str = match std::str::from_utf8(callee_name) {
			Ok(s) => s,
			Err(_) => return,
		};
		if !self
			.presets
			.di_register_callees
			.iter()
			.any(|p| p == callee_str)
		{
			return;
		}
		let Some(args) = call.child_by_field_name("arguments") else {
			return;
		};
		let mut cursor = args.walk();
		for c in args.children(&mut cursor) {
			if !c.is_named() {
				continue;
			}
			if let Some(name) = self.find_di_factory(c) {
				let target = extend_segment(&self.module, kinds::CLASS, name);
				let attrs = RefAttrs {
					confidence: kinds::CONF_NAME_MATCH,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::DI_REGISTER, Some(pos), &attrs);
			}
		}
	}

	fn find_di_factory<'a>(&'a self, node: Node<'a>) -> Option<&'a [u8]> {
		match node.kind() {
			"identifier" => {
				let name = node_slice(node, self.source_bytes);
				(!name.is_empty()).then_some(name)
			}
			"call_expression" => {
				let fn_node = node.child_by_field_name("function")?;
				match fn_node.kind() {
					"member_expression" => fn_node
						.child_by_field_name("object")
						.and_then(|obj| self.find_di_factory(obj)),
					"identifier" => {
						let inner_args = node.child_by_field_name("arguments")?;
						let mut cur = inner_args.walk();
						for c in inner_args.children(&mut cur) {
							if !c.is_named() {
								continue;
							}
							if let Some(name) = self.find_di_factory(c) {
								return Some(name);
							}
						}
						None
					}
					_ => None,
				}
			}
			_ => None,
		}
	}

	fn recurse_subtree(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let walker = crate::lang::canonical_walker::CanonicalWalker::new(self, self.source_bytes);
		walker.dispatch(node, scope, graph);
	}

	fn walk_children(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let walker = crate::lang::canonical_walker::CanonicalWalker::new(self, self.source_bytes);
		walker.walk(node, scope, graph);
	}

	fn push_local_scope(&self) {
		self.local_scope.borrow_mut().push(HashSet::new());
	}

	fn pop_local_scope(&self) {
		self.local_scope.borrow_mut().pop();
	}

	fn record_local(&self, name: &[u8]) {
		if let Some(top) = self.local_scope.borrow_mut().last_mut() {
			top.insert(name.to_vec());
		}
	}

	fn is_local_name(&self, name: &[u8]) -> bool {
		self.local_scope
			.borrow()
			.iter()
			.any(|frame| frame.contains(name))
	}

	fn name_confidence(&self, name: &[u8]) -> Option<&'static [u8]> {
		crate::lang::kinds::name_confidence_for(self.is_local_name(name), self.deep)
	}

	fn module_visibility(&self, node: Node<'_>) -> &'static [u8] {
		let start = node.start_byte() as u32;
		if self
			.export_ranges
			.iter()
			.any(|(a, b)| *a <= start && start < *b)
		{
			kinds::VIS_PUBLIC
		} else {
			kinds::VIS_MODULE
		}
	}
}

pub(super) fn collect_export_ranges(root: Node<'_>) -> Vec<(u32, u32)> {
	let mut out = Vec::new();
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() == "export_statement" {
			out.push((child.start_byte() as u32, child.end_byte() as u32));
		}
	}
	out
}

fn is_callable_kind(kind: &[u8]) -> bool {
	kind == kinds::FUNCTION || kind == kinds::METHOD || kind == kinds::CONSTRUCTOR
}

fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::FUNCTION || last.kind == kinds::METHOD || last.kind == kinds::CONSTRUCTOR
}

fn class_member_visibility(node: Node<'_>, source: &[u8]) -> &'static [u8] {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "accessibility_modifier" {
			return match c.utf8_text(source).unwrap_or("") {
				"private" => kinds::VIS_PRIVATE,
				"protected" => kinds::VIS_PROTECTED,
				_ => kinds::VIS_PUBLIC,
			};
		}
	}
	kinds::VIS_PUBLIC
}

fn collect_binding_names(pat: Node<'_>, source: &[u8]) -> Vec<Vec<u8>> {
	fn rec(node: Node<'_>, source: &[u8], out: &mut Vec<Vec<u8>>) {
		match node.kind() {
			"identifier" | "shorthand_property_identifier_pattern" => {
				let slice = &source[node.start_byte()..node.end_byte().min(source.len())];
				out.push(slice.to_vec());
			}
			"object_pattern" | "array_pattern" | "pair_pattern" | "rest_pattern"
			| "assignment_pattern" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					rec(c, source, out);
				}
			}
			_ => {}
		}
	}
	let mut out = Vec::new();
	rec(pat, source, &mut out);
	out
}

fn call_argument_count(call: Node<'_>) -> u16 {
	let Some(args) = call.child_by_field_name("arguments") else {
		return 0;
	};
	let mut cursor = args.walk();
	let mut count: u16 = 0;
	for c in args.children(&mut cursor) {
		if c.is_named() {
			count = count.saturating_add(1);
		}
	}
	count
}

fn receiver_hint<'a>(member_expr: Node<'_>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SUBSCRIPT, HINT_SUPER, HINT_THIS};
	let Some(obj) = member_expr.child_by_field_name("object") else {
		return b"";
	};
	match obj.kind() {
		"this" => HINT_THIS,
		"super" => HINT_SUPER,
		"identifier" => obj.utf8_text(source).unwrap_or("").as_bytes(),
		"call_expression" => HINT_CALL,
		"member_expression" => HINT_MEMBER,
		"subscript_expression" => HINT_SUBSCRIPT,
		_ => b"",
	}
}

fn generic_short(node: Node<'_>, source: &[u8]) -> Option<String> {
	let inner = node.child_by_field_name("name").or_else(|| {
		let mut cursor = node.walk();
		node.named_children(&mut cursor).next()
	})?;
	match inner.kind() {
		"type_identifier" => inner.utf8_text(source).ok().map(|s| s.to_string()),
		"nested_type_identifier" => nested_type_short(inner, source),
		_ => inner.utf8_text(source).ok().map(|s| s.to_string()),
	}
}

fn nested_type_short(node: Node<'_>, source: &[u8]) -> Option<String> {
	if let Some(name) = node.child_by_field_name("name") {
		return name.utf8_text(source).ok().map(|s| s.to_string());
	}
	let mut cursor = node.walk();
	let mut last: Option<String> = None;
	for c in node.named_children(&mut cursor) {
		if c.kind() == "type_identifier" || c.kind() == "identifier" {
			last = c.utf8_text(source).ok().map(|s| s.to_string());
		}
	}
	last
}

fn is_relative_specifier(spec: &str) -> bool {
	spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../")
}

fn import_confidence(spec: &str) -> &'static [u8] {
	if is_relative_specifier(spec) {
		kinds::CONF_IMPORTED
	} else {
		kinds::CONF_EXTERNAL
	}
}

fn first_identifier_text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a [u8] {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "identifier" {
			return &source[c.start_byte()..c.end_byte().min(source.len())];
		}
	}
	b""
}

fn unquote_string_literal<'src>(node: Node<'_>, source: &'src [u8]) -> &'src str {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "string_fragment"
			&& let Ok(s) = c.utf8_text(source)
		{
			return s;
		}
	}
	node.utf8_text(source)
		.unwrap_or("")
		.trim_matches(|c| c == '"' || c == '\'' || c == '`')
}
