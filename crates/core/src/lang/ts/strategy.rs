use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{
	callable_segment_slots, extend_callable_slots, extend_segment, join_bytes_with_comma,
	slot_signature_bytes,
};
use crate::lang::strategy::{LangStrategy, NodeShape, RefSpec, Symbol};
use crate::lang::tree_util::{find_named_child, node_position, node_slice};

use super::canonicalize::{
	anonymous_callback_name, append_module_segments, callable_param_slots, external_pkg_builder,
	strip_known_extension,
};
use super::kinds;

struct DecoratorCallee<'src> {
	name: &'src [u8],
	args: Option<Node<'src>>,
}

pub(super) struct Strategy<'src> {
	pub(super) module: Moniker,
	pub(super) anchor: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) presets: &'src super::Presets,
	pub(super) export_ranges: Vec<(u32, u32)>,
	pub(super) local_scope: RefCell<Vec<HashSet<Vec<u8>>>>,
	pub(super) imports: RefCell<HashMap<Vec<u8>, &'static [u8]>>,
	pub(super) callable_table: HashMap<(Moniker, Vec<u8>), Vec<u8>>,
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
			"method_definition" | "method_signature" => self.classify_method(node, scope, source),
			"public_field_definition" | "property_signature" => {
				self.classify_field(node, scope, source)
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
				self.dispatch_fields(node, scope, graph, &["left", "right"]);
				NodeShape::Skip
			}
			"unary_expression" | "update_expression" => {
				self.dispatch_fields(node, scope, graph, &["argument"]);
				NodeShape::Skip
			}
			"ternary_expression" => {
				self.dispatch_fields(
					node,
					scope,
					graph,
					&["condition", "consequence", "alternative"],
				);
				NodeShape::Skip
			}
			"member_expression" | "subscript_expression" => {
				self.dispatch_fields(node, scope, graph, &["object"]);
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
			self.bind_and_emit_params(params, moniker, graph);
		}
		if let Some(p) = node.child_by_field_name("parameter") {
			self.bind_and_emit_param_leaf(p, moniker, graph);
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
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"decorator" => self.collect_decorator_ref(child, &mut annotated_by),
				"class_heritage" => {
					self.collect_heritage_refs_from_clauses(child, &mut annotated_by)
				}
				_ => {}
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
		self.callable_symbol(node, node, name, kind, scope, vis)
	}

	fn classify_field<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
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
		let slots = callable_param_slots(callable_node, self.source_bytes);
		let _signature =
			join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
		let moniker = extend_callable_slots(scope, kind, name, &slots);

		self.push_local_scope();

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

	fn bind_and_emit_params(&self, params: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"required_parameter" | "optional_parameter" => {
					if let Some(p) = child.child_by_field_name("pattern") {
						self.bind_and_emit_param_leaf(p, callable, graph);
					}
					if let Some(t) = child.child_by_field_name("type") {
						self.emit_uses_type_recursive(t, callable, graph);
					}
				}
				"rest_pattern" => {
					self.bind_and_emit_param_leaf(child, callable, graph);
				}
				_ => {}
			}
		}
	}

	fn bind_and_emit_param_leaf(&self, pat: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		for name in collect_binding_names(pat, self.source_bytes) {
			self.record_local(&name);
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
				let slots = callable_param_slots(v, self.source_bytes);
				let m = extend_callable_slots(scope, kinds::FUNCTION, name, &slots);
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
					self.bind_and_emit_params(params, &m, graph);
				}
				if let Some(p) = v.child_by_field_name("parameter") {
					self.bind_and_emit_param_leaf(p, &m, graph);
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
			self.bind_and_emit_param_leaf(p, scope, graph);
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
							self.lookup_callable(name, kinds::FUNCTION)
								.unwrap_or_else(|| {
									extend_segment(&self.module, kinds::FUNCTION, name)
								})
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
						let target = extend_segment(&self.module, kinds::METHOD, name);
						let confidence = fn_node
							.child_by_field_name("object")
							.filter(|o| o.kind() == "identifier")
							.map(|o| self.ref_confidence(node_slice(o, self.source_bytes)))
							.unwrap_or(kinds::CONF_NAME_MATCH);
						let attrs = RefAttrs {
							receiver_hint: receiver_hint(fn_node, self.source_bytes),
							confidence,
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
				let confidence = match ctor.kind() {
					"identifier" | "type_identifier" => self.ref_confidence(n),
					"member_expression" => ctor
						.child_by_field_name("object")
						.filter(|o| o.kind() == "identifier")
						.map(|o| self.ref_confidence(node_slice(o, self.source_bytes)))
						.unwrap_or(kinds::CONF_NAME_MATCH),
					_ => kinds::CONF_NAME_MATCH,
				};
				let attrs = RefAttrs {
					confidence,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::INSTANTIATES, Some(pos), &attrs);
			}
		}
		self.walk_children(node, scope, graph);
	}

	fn decorator_callees<'src>(&self, decorator: Node<'src>) -> Vec<DecoratorCallee<'src>>
	where
		'src_lang: 'src,
	{
		let mut out = Vec::new();
		let mut cursor = decorator.walk();
		for ch in decorator.children(&mut cursor) {
			match ch.kind() {
				"identifier" => {
					let name = node_slice(ch, self.source_bytes);
					if !name.is_empty() {
						out.push(DecoratorCallee { name, args: None });
					}
				}
				"call_expression" => {
					if let Some(fn_node) = ch.child_by_field_name("function")
						&& fn_node.kind() == "identifier"
					{
						let name = node_slice(fn_node, self.source_bytes);
						if !name.is_empty() {
							out.push(DecoratorCallee {
								name,
								args: ch.child_by_field_name("arguments"),
							});
						}
					}
				}
				_ => {}
			}
		}
		out
	}

	fn handle_decorator(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		for callee in self.decorator_callees(node) {
			let target = extend_segment(&self.module, kinds::FUNCTION, callee.name);
			let attrs = RefAttrs {
				confidence: self.ref_confidence(callee.name),
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::ANNOTATES, Some(pos), &attrs);
			if let Some(args) = callee.args {
				self.walk_children(args, scope, graph);
			}
		}
	}

	fn walk_decorator_args(
		&self,
		decorator: Node<'_>,
		sym_moniker: &Moniker,
		graph: &mut CodeGraph,
	) {
		for callee in self.decorator_callees(decorator) {
			if let Some(args) = callee.args {
				self.walk_children(args, sym_moniker, graph);
			}
		}
	}

	fn collect_decorator_ref(&self, decorator: Node<'_>, out: &mut Vec<RefSpec>) {
		let pos = node_position(decorator);
		for callee in self.decorator_callees(decorator) {
			let target = extend_segment(&self.module, kinds::FUNCTION, callee.name);
			out.push(RefSpec {
				kind: kinds::ANNOTATES,
				target,
				confidence: self.ref_confidence(callee.name),
				position: pos,
				receiver_hint: b"",
				alias: b"",
			});
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
			let name: Option<&[u8]> = match c.kind() {
				"identifier" | "type_identifier" => Some(node_slice(c, self.source_bytes)),
				"member_expression" => c
					.child_by_field_name("property")
					.map(|p| node_slice(p, self.source_bytes)),
				"generic_type" => generic_short(c, self.source_bytes),
				"nested_type_identifier" => nested_type_short(c, self.source_bytes),
				_ => None,
			};
			let Some(name) = name.filter(|n| !n.is_empty()) else {
				continue;
			};
			let target = extend_segment(&self.module, target_kind, name);
			out.push(RefSpec {
				kind: edge,
				target,
				confidence: self.ref_confidence(name),
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
					confidence: self.ref_confidence(name),
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
					let target = extend_segment(&self.module, kinds::CLASS, name);
					let root = nested_type_root(node, self.source_bytes).unwrap_or(name);
					let attrs = RefAttrs {
						confidence: self.ref_confidence(root),
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
					let target = extend_segment(&self.module, kinds::CLASS, name);
					let attrs = RefAttrs {
						confidence: self.ref_confidence(name),
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
			extend_segment(&self.module, kinds::FUNCTION, name)
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

	fn dispatch_fields(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		fields: &[&str],
	) {
		for f in fields {
			let Some(c) = node.child_by_field_name(f) else {
				continue;
			};
			if c.kind() == "identifier" {
				self.emit_read_at(c, scope, graph);
			} else {
				self.recurse_subtree(c, scope, graph);
			}
		}
	}

	fn handle_jsx_element(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(name) = node.child_by_field_name("name")
			&& name.kind() == "identifier"
			&& !is_intrinsic_jsx_tag(node_slice(name, self.source_bytes))
		{
			self.emit_read_at(name, scope, graph);
		}
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"jsx_attribute" => {
					if let Some(v) = c.child_by_field_name("value") {
						self.recurse_subtree(v, scope, graph);
					}
				}
				"jsx_text" => {}
				_ => self.recurse_subtree(c, scope, graph),
			}
		}
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

		let confidence = import_confidence(raw_spec);
		let Some(clause) = find_named_child(node, "import_clause") else {
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
					self.record_import(local_name, confidence);
					let target = self.import_symbol_target(raw_spec, b"default");
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
					self.record_import(alias, confidence);
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
						let Some(name) = spec
							.child_by_field_name("name")
							.map(|n| node_slice(n, self.source_bytes))
							.filter(|n| !n.is_empty())
						else {
							continue;
						};
						let alias = spec
							.child_by_field_name("alias")
							.map(|n| node_slice(n, self.source_bytes))
							.unwrap_or(b"");
						let local = if alias.is_empty() { name } else { alias };
						self.record_import(local, confidence);
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
			let Some(name) = spec
				.child_by_field_name("name")
				.map(|n| node_slice(n, self.source_bytes))
				.filter(|n| !n.is_empty())
			else {
				continue;
			};
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

	fn import_symbol_target(&self, raw_path: &str, name: &[u8]) -> Moniker {
		self.import_target(raw_path, Some(name))
	}

	fn import_target(&self, raw_path: &str, symbol: Option<&[u8]>) -> Moniker {
		let mut b = if let Some(resolved) = self.resolve_path_alias(raw_path) {
			self.project_rooted_module_builder(&resolved)
		} else if is_relative_specifier(raw_path) {
			self.relative_module_builder(raw_path)
		} else {
			external_pkg_builder(self.module.as_view().project(), raw_path)
		};
		if let Some(sym) = symbol {
			b.segment(kinds::PATH, sym);
		}
		b.build()
	}

	fn resolve_path_alias(&self, spec: &str) -> Option<String> {
		for alias in &self.presets.path_aliases {
			if let Some(captured) = match_path_alias(&alias.pattern, spec) {
				return Some(apply_path_alias(&alias.substitution, captured));
			}
		}
		None
	}

	fn project_rooted_module_builder(&self, path: &str) -> MonikerBuilder {
		super::canonicalize::module_builder_for_path(&self.anchor, path)
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
		if self.is_local_name(name) {
			return if self.deep {
				Some(kinds::CONF_LOCAL)
			} else {
				None
			};
		}
		Some(
			self.import_confidence_for(name)
				.unwrap_or(kinds::CONF_NAME_MATCH),
		)
	}

	fn import_confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.imports.borrow().get(name).copied()
	}

	fn record_import(&self, name: &[u8], confidence: &'static [u8]) {
		if name.is_empty() {
			return;
		}
		self.imports.borrow_mut().insert(name.to_vec(), confidence);
	}

	fn lookup_callable(&self, name: &[u8], kind: &[u8]) -> Option<Moniker> {
		let seg = self
			.callable_table
			.get(&(self.module.clone(), name.to_vec()))?;
		Some(extend_segment(&self.module, kind, seg))
	}

	fn ref_confidence(&self, name: &[u8]) -> &'static [u8] {
		self.import_confidence_for(name)
			.unwrap_or(kinds::CONF_NAME_MATCH)
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

pub(super) fn collect_callable_table<'src>(
	root: Node<'src>,
	source: &'src [u8],
	module: &Moniker,
	out: &mut HashMap<(Moniker, Vec<u8>), Vec<u8>>,
) {
	visit_top_level(root, |child| match child.kind() {
		"function_declaration" | "generator_function_declaration" => {
			if let Some(name_node) = child.child_by_field_name("name") {
				let name = node_slice(name_node, source);
				let slots = callable_param_slots(child, source);
				let seg = callable_segment_slots(name, &slots);
				out.insert((module.clone(), name.to_vec()), seg);
			}
		}
		"lexical_declaration" | "variable_declaration" => {
			let mut nc = child.walk();
			for decl in child.named_children(&mut nc) {
				if decl.kind() != "variable_declarator" {
					continue;
				}
				let Some(name_node) = decl.child_by_field_name("name") else {
					continue;
				};
				if name_node.kind() != "identifier" {
					continue;
				}
				let name = node_slice(name_node, source);
				if let Some(value) = decl.child_by_field_name("value")
					&& matches!(value.kind(), "arrow_function" | "function_expression")
				{
					let slots = callable_param_slots(value, source);
					let seg = callable_segment_slots(name, &slots);
					out.insert((module.clone(), name.to_vec()), seg);
				}
			}
		}
		_ => {}
	});
}

fn visit_top_level<'src, F: FnMut(Node<'src>)>(root: Node<'src>, mut f: F) {
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		match child.kind() {
			"export_statement" => {
				let mut ec = child.walk();
				for inner in child.named_children(&mut ec) {
					f(inner);
				}
			}
			_ => f(child),
		}
	}
}

pub(super) fn collect_export_ranges(root: Node<'_>) -> Vec<(u32, u32)> {
	let mut out = Vec::new();
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() == "export_statement" {
			out.push(node_position(child));
		}
	}
	out
}

fn is_callable_kind(kind: &[u8]) -> bool {
	kind == kinds::FUNCTION || kind == kinds::METHOD || kind == kinds::CONSTRUCTOR
}

fn is_intrinsic_jsx_tag(name: &[u8]) -> bool {
	matches!(name.first(), Some(b'a'..=b'z'))
}

fn match_path_alias<'a>(pattern: &str, spec: &'a str) -> Option<&'a str> {
	if let Some(star) = pattern.find('*') {
		let prefix = &pattern[..star];
		let suffix = &pattern[star + 1..];
		if spec.len() >= prefix.len() + suffix.len()
			&& spec.starts_with(prefix)
			&& spec.ends_with(suffix)
		{
			return Some(&spec[prefix.len()..spec.len() - suffix.len()]);
		}
		None
	} else if pattern == spec {
		Some("")
	} else {
		None
	}
}

fn apply_path_alias(template: &str, captured: &str) -> String {
	if let Some(star) = template.find('*') {
		let mut out = String::with_capacity(template.len() + captured.len());
		out.push_str(&template[..star]);
		out.push_str(captured);
		out.push_str(&template[star + 1..]);
		out
	} else {
		template.to_string()
	}
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

fn collect_binding_names<'src>(pat: Node<'src>, source: &'src [u8]) -> Vec<Vec<u8>> {
	fn rec<'src>(node: Node<'src>, source: &'src [u8], out: &mut Vec<Vec<u8>>) {
		match node.kind() {
			"identifier" | "shorthand_property_identifier_pattern" => {
				out.push(node_slice(node, source).to_vec());
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

fn generic_short<'src>(node: Node<'src>, source: &'src [u8]) -> Option<&'src [u8]> {
	let inner = node.child_by_field_name("name").or_else(|| {
		let mut cursor = node.walk();
		node.named_children(&mut cursor).next()
	})?;
	match inner.kind() {
		"nested_type_identifier" => nested_type_short(inner, source),
		_ => Some(node_slice(inner, source)),
	}
}

fn nested_type_short<'src>(node: Node<'src>, source: &'src [u8]) -> Option<&'src [u8]> {
	if let Some(name) = node.child_by_field_name("name") {
		return Some(node_slice(name, source));
	}
	let mut cursor = node.walk();
	let mut last: Option<&'src [u8]> = None;
	for c in node.named_children(&mut cursor) {
		if c.kind() == "type_identifier" || c.kind() == "identifier" {
			last = Some(node_slice(c, source));
		}
	}
	last
}

fn nested_type_root<'src>(node: Node<'src>, source: &'src [u8]) -> Option<&'src [u8]> {
	let mut cursor = node.walk();
	for c in node.named_children(&mut cursor) {
		match c.kind() {
			"type_identifier" | "identifier" => return Some(node_slice(c, source)),
			"nested_type_identifier" => return nested_type_root(c, source),
			_ => {}
		}
	}
	None
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

fn first_identifier_text<'src>(node: Node<'src>, source: &'src [u8]) -> &'src [u8] {
	find_named_child(node, "identifier")
		.map(|c| node_slice(c, source))
		.unwrap_or(b"")
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
