use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{
	callable_segment_slots, extend_callable_slots, extend_segment, join_bytes_with_comma,
	slot_signature_bytes,
};
use crate::lang::strategy::{LangStrategy, NodeShape, Symbol};
use crate::lang::tree_util::{node_position, node_slice};

use super::canonicalize::function_param_slots;
use super::kinds;

#[derive(Clone, Debug)]
pub(super) struct ImportEntry {
	pub confidence: &'static [u8],
	pub module_prefix: Moniker,
}

pub(super) struct Strategy<'src> {
	pub(super) module: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) imports: RefCell<HashMap<Vec<u8>, ImportEntry>>,
	pub(super) local_scope: RefCell<Vec<HashSet<Vec<u8>>>>,
	pub(super) type_table: HashMap<&'src [u8], Moniker>,
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
			"package_clause" => NodeShape::Skip,
			"comment" => NodeShape::Annotation {
				kind: kinds::COMMENT,
			},
			"import_declaration" => {
				self.handle_import(node, scope, graph);
				NodeShape::Skip
			}
			"function_declaration" => self.classify_function(node, scope, source),
			"method_declaration" => self.classify_method(node, source),
			"type_declaration" => NodeShape::Recurse,
			"type_spec" => {
				self.handle_type_spec(node, scope, source, graph);
				NodeShape::Skip
			}
			"type_alias" => {
				self.handle_type_alias(node, scope, source, graph);
				NodeShape::Skip
			}
			"call_expression" => {
				self.handle_call(node, scope, graph);
				NodeShape::Skip
			}
			"composite_literal" => {
				self.handle_composite_literal(node, scope, graph);
				NodeShape::Skip
			}
			"short_var_declaration" => {
				self.handle_short_var_declaration(node, scope, graph);
				NodeShape::Skip
			}
			"var_declaration" => {
				self.handle_var_declaration(node, scope, graph);
				NodeShape::Skip
			}
			"const_declaration" => {
				self.handle_const_declaration(node, scope, graph);
				NodeShape::Skip
			}
			"range_clause" => {
				self.handle_range_clause(node, scope, graph);
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
		source: &[u8],
		graph: &mut CodeGraph,
	) {
		if kind != kinds::FUNC && kind != kinds::METHOD {
			return;
		}
		if let Some(receiver) = node.child_by_field_name("receiver") {
			self.emit_param_defs(receiver, moniker, source, graph);
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			self.emit_param_defs(params, moniker, source, graph);
			self.emit_param_type_refs(params, moniker, graph);
		}
		if let Some(result) = node.child_by_field_name("result") {
			self.emit_uses_type(result, moniker, graph);
		}
	}

	fn after_body(&self, kind: &[u8], _moniker: &Moniker) {
		if kind == kinds::FUNC || kind == kinds::METHOD {
			self.pop_local_scope();
		}
	}
}

impl<'src_lang> Strategy<'src_lang> {
	fn classify_function<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let slots = function_param_slots(node, source);
		let signature =
			join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
		let moniker = extend_callable_slots(scope, kinds::FUNC, name, &slots);

		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			self.record_param_locals(params, source);
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::FUNC,
			visibility: visibility_from_name(name),
			signature: Some(signature),
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn classify_method<'src>(&self, node: Node<'src>, source: &'src [u8]) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let Some(receiver) = node.child_by_field_name("receiver") else {
			return NodeShape::Recurse;
		};
		let Some(receiver_name) = receiver_type_name(receiver, source) else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let owner = self
			.type_table
			.get(receiver_name)
			.cloned()
			.unwrap_or_else(|| extend_segment(&self.module, kinds::STRUCT, receiver_name));
		let slots = function_param_slots(node, source);
		let signature =
			join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
		let moniker = extend_callable_slots(&owner, kinds::METHOD, name, &slots);

		self.push_local_scope();
		self.record_param_locals(receiver, source);
		if let Some(params) = node.child_by_field_name("parameters") {
			self.record_param_locals(params, source);
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::METHOD,
			visibility: visibility_from_name(name),
			signature: Some(signature),
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn record_param_locals(&self, container: Node<'_>, source: &[u8]) {
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
				if n.kind() == "identifier" {
					let s = node_slice(n, source);
					if !s.is_empty() && s != b"_" {
						self.record_local(s);
					}
				}
			}
		}
	}

	fn emit_param_defs(
		&self,
		container: Node<'_>,
		callable: &Moniker,
		source: &[u8],
		graph: &mut CodeGraph,
	) {
		if !self.deep {
			return;
		}
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
				if n.kind() == "identifier" {
					let s = node_slice(n, source);
					if !s.is_empty() && s != b"_" {
						let m = extend_segment(callable, kinds::PARAM, s);
						let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(n)));
					}
				}
			}
		}
	}

	fn emit_param_type_refs(&self, params: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = params.walk();
		for c in params.named_children(&mut cursor) {
			if let Some(t) = c.child_by_field_name("type") {
				self.emit_uses_type(t, scope, graph);
			}
		}
	}

	fn handle_type_spec(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		source: &[u8],
		graph: &mut CodeGraph,
	) {
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let Some(type_node) = node.child_by_field_name("type") else {
			return;
		};
		let name = node_slice(name_node, source);
		let kind = match type_node.kind() {
			"struct_type" => kinds::STRUCT,
			"interface_type" => kinds::INTERFACE,
			_ => kinds::TYPE,
		};
		let moniker = extend_segment(scope, kind, name);
		if scope != &self.module {
			let attrs = DefAttrs {
				visibility: visibility_from_name(name),
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(
				moniker.clone(),
				kind,
				scope,
				Some(node_position(node)),
				&attrs,
			);
		}
		match type_node.kind() {
			"struct_type" => self.emit_struct_body(type_node, &moniker, graph),
			"interface_type" => self.emit_interface_body(type_node, &moniker, graph),
			_ => self.emit_uses_type(type_node, &moniker, graph),
		}
	}

	fn handle_type_alias(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		source: &[u8],
		graph: &mut CodeGraph,
	) {
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::TYPE, name);
		if scope != &self.module {
			let attrs = DefAttrs {
				visibility: visibility_from_name(name),
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(
				moniker.clone(),
				kinds::TYPE,
				scope,
				Some(node_position(node)),
				&attrs,
			);
		}
		if let Some(type_node) = node.child_by_field_name("type") {
			self.emit_uses_type(type_node, &moniker, graph);
		}
	}

	fn handle_short_var_declaration(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if is_callable_scope(scope, &self.module)
			&& let Some(left) = node.child_by_field_name("left")
		{
			self.bind_locals_in_expression_list(left, scope, graph);
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.recurse_subtree(right, scope, graph);
		}
	}

	fn handle_var_declaration(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.handle_var_or_const(node, scope, graph, "var_spec", kinds::VAR);
	}

	fn handle_const_declaration(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.handle_var_or_const(node, scope, graph, "const_spec", kinds::CONST);
	}

	fn handle_var_or_const(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		spec_kind: &str,
		def_kind: &'static [u8],
	) {
		let in_callable = is_callable_scope(scope, &self.module);
		for spec in spec_children(node, spec_kind) {
			if let Some(t) = spec.child_by_field_name("type") {
				self.emit_uses_type(t, scope, graph);
			}
			let mut nc = spec.walk();
			for n in spec.named_children(&mut nc) {
				if n.kind() != "identifier" {
					continue;
				}
				let s = node_slice(n, self.source_bytes);
				if s.is_empty() || s == b"_" {
					continue;
				}
				if in_callable {
					self.record_local(s);
					if self.deep {
						let m = extend_segment(scope, kinds::LOCAL, s);
						let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(n)));
					}
				} else if scope == &self.module {
					let m = extend_segment(scope, def_kind, s);
					let attrs = DefAttrs {
						visibility: visibility_from_name(s),
						..DefAttrs::default()
					};
					let _ = graph.add_def_attrs(m, def_kind, scope, Some(node_position(n)), &attrs);
				}
			}
			if let Some(value) = spec.child_by_field_name("value") {
				self.recurse_subtree(value, scope, graph);
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
			self.recurse_subtree(right, scope, graph);
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
				let s = node_slice(node, self.source_bytes);
				if s.is_empty() || s == b"_" {
					return;
				}
				self.record_local(s);
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, s);
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

	fn handle_import(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			match child.kind() {
				"import_spec" => self.handle_import_spec(child, scope, graph),
				"import_spec_list" => {
					let mut sc = child.walk();
					for spec in child.named_children(&mut sc) {
						if spec.kind() == "import_spec" {
							self.handle_import_spec(spec, scope, graph);
						}
					}
				}
				_ => {}
			}
		}
	}

	fn handle_import_spec(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(path_node) = node.child_by_field_name("path") else {
			return;
		};
		let raw_bytes = node_slice(path_node, self.source_bytes);
		let raw = std::str::from_utf8(raw_bytes).unwrap_or("");
		let path = strip_string_quotes(raw);
		if path.is_empty() {
			return;
		}
		let pieces: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
		if pieces.is_empty() {
			return;
		}

		let alias_text = node
			.child_by_field_name("name")
			.map(|n| node_slice(n, self.source_bytes))
			.and_then(|b| std::str::from_utf8(b).ok())
			.unwrap_or("");

		let confidence = stdlib_or_imported(&pieces);

		let bind: Option<&[u8]> = match alias_text {
			"" => pieces.last().copied().map(str::as_bytes),
			"." | "_" => None,
			other => Some(other.as_bytes()),
		};
		let module_prefix = build_module_target(self.module.as_view().project(), &pieces);
		if let Some(b) = bind
			&& !b.is_empty()
		{
			self.imports.borrow_mut().insert(
				b.to_vec(),
				ImportEntry {
					confidence,
					module_prefix: module_prefix.clone(),
				},
			);
		}

		let attrs = RefAttrs {
			confidence,
			alias: alias_text.as_bytes(),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			module_prefix,
			kinds::IMPORTS_MODULE,
			Some(node_position(node)),
			&attrs,
		);
	}

	fn handle_call(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let Some(callee) = node.child_by_field_name("function") else {
			self.recurse_subtree(node, scope, graph);
			return;
		};
		match callee.kind() {
			"identifier" => self.emit_simple_call(callee, scope, pos, graph),
			"selector_expression" => self.emit_selector_call(callee, scope, pos, graph),
			_ => self.recurse_subtree(callee, scope, graph),
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.recurse_subtree(args, scope, graph);
		}
	}

	fn emit_simple_call(
		&self,
		callee: Node<'_>,
		scope: &Moniker,
		pos: (u32, u32),
		graph: &mut CodeGraph,
	) {
		let name = node_slice(callee, self.source_bytes);
		if name.is_empty() {
			return;
		}
		if let Some(entry) = self.import_entry_for(name) {
			let target = extend_segment(&entry.module_prefix, kinds::FUNC, name);
			let attrs = RefAttrs {
				confidence: entry.confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
			return;
		}
		let Some(conf) = self.name_confidence(name) else {
			return;
		};
		let target = if conf == kinds::CONF_LOCAL {
			extend_segment(scope, kinds::LOCAL, name)
		} else {
			self.lookup_module_callable(name, kinds::FUNC)
				.unwrap_or_else(|| extend_segment(&self.module, kinds::FUNC, name))
		};
		let attrs = RefAttrs {
			confidence: conf,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
	}

	fn emit_selector_call(
		&self,
		callee: Node<'_>,
		scope: &Moniker,
		pos: (u32, u32),
		graph: &mut CodeGraph,
	) {
		let Some(field_node) = callee.child_by_field_name("field") else {
			self.recurse_subtree(callee, scope, graph);
			return;
		};
		let name = node_slice(field_node, self.source_bytes);
		if name.is_empty() {
			return;
		}
		let operand = callee.child_by_field_name("operand");

		if let Some(op) = operand
			&& op.kind() == "identifier"
		{
			let op_name = node_slice(op, self.source_bytes);
			if let Some(entry) = self.import_entry_for(op_name) {
				let target = extend_segment(&entry.module_prefix, kinds::FUNC, name);
				let attrs = RefAttrs {
					confidence: entry.confidence,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
				return;
			}
		}

		let target = extend_segment(&self.module, kinds::METHOD, name);
		let hint = operand
			.map(|o| receiver_hint(o, self.source_bytes))
			.unwrap_or(b"");
		let attrs = RefAttrs {
			receiver_hint: hint,
			confidence: kinds::CONF_NAME_MATCH,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::METHOD_CALL, Some(pos), &attrs);

		if let Some(op) = operand {
			self.recurse_subtree(op, scope, graph);
		}
	}

	fn handle_composite_literal(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		if let Some(type_node) = node.child_by_field_name("type") {
			self.emit_instantiates(type_node, scope, pos, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.recurse_subtree(body, scope, graph);
		}
	}

	fn emit_instantiates(
		&self,
		type_node: Node<'_>,
		scope: &Moniker,
		pos: (u32, u32),
		graph: &mut CodeGraph,
	) {
		match type_node.kind() {
			"type_identifier" | "qualified_type" => {
				self.emit_resolved_type_ref(type_node, scope, kinds::INSTANTIATES, pos, graph);
			}
			"generic_type" => {
				if let Some(inner) = type_node.child_by_field_name("type") {
					self.emit_instantiates(inner, scope, pos, graph);
				}
			}
			_ => {}
		}
	}

	fn resolve_type_node(&self, type_node: Node<'_>) -> Option<(Moniker, &'static [u8])> {
		match type_node.kind() {
			"type_identifier" => {
				let name = node_slice(type_node, self.source_bytes);
				if name.is_empty() || is_go_primitive(name) {
					return None;
				}
				Some(self.resolve_type_target(name, kinds::STRUCT))
			}
			"qualified_type" => {
				let pkg = type_node
					.child_by_field_name("package")
					.map(|n| node_slice(n, self.source_bytes))
					.unwrap_or(b"");
				let name_node = type_node.child_by_field_name("name")?;
				let name = node_slice(name_node, self.source_bytes);
				if name.is_empty() {
					return None;
				}
				if let Some(entry) = self.import_entry_for(pkg) {
					Some((
						extend_segment(&entry.module_prefix, kinds::STRUCT, name),
						entry.confidence,
					))
				} else {
					Some(self.resolve_type_target(name, kinds::STRUCT))
				}
			}
			_ => None,
		}
	}

	fn emit_resolved_type_ref(
		&self,
		type_node: Node<'_>,
		scope: &Moniker,
		ref_kind: &[u8],
		pos: (u32, u32),
		graph: &mut CodeGraph,
	) {
		if let Some((target, confidence)) = self.resolve_type_node(type_node) {
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, ref_kind, Some(pos), &attrs);
		}
	}

	fn emit_uses_type(&self, type_node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		match type_node.kind() {
			"type_identifier" | "qualified_type" => {
				self.emit_resolved_type_ref(
					type_node,
					scope,
					kinds::USES_TYPE,
					node_position(type_node),
					graph,
				);
			}
			"pointer_type" | "slice_type" | "array_type" | "channel_type" | "map_type"
			| "parenthesized_type" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					self.emit_uses_type(c, scope, graph);
				}
			}
			"generic_type" => {
				if let Some(head) = type_node.child_by_field_name("type") {
					self.emit_uses_type(head, scope, graph);
				}
				if let Some(args) = type_node.child_by_field_name("type_arguments") {
					let mut cursor = args.walk();
					for c in args.named_children(&mut cursor) {
						self.emit_uses_type(c, scope, graph);
					}
				}
			}
			"function_type" => {
				if let Some(params) = type_node.child_by_field_name("parameters") {
					let mut cursor = params.walk();
					for c in params.named_children(&mut cursor) {
						if let Some(t) = c.child_by_field_name("type") {
							self.emit_uses_type(t, scope, graph);
						}
					}
				}
				if let Some(result) = type_node.child_by_field_name("result") {
					self.emit_uses_type(result, scope, graph);
				}
			}
			"parameter_list" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					if let Some(t) = c.child_by_field_name("type") {
						self.emit_uses_type(t, scope, graph);
					}
				}
			}
			"struct_type" | "interface_type" => {
				self.recurse_subtree(type_node, scope, graph);
			}
			_ => {}
		}
	}

	fn emit_struct_body(&self, struct_node: Node<'_>, owner: &Moniker, graph: &mut CodeGraph) {
		let Some(field_list) = struct_field_list(struct_node) else {
			return;
		};
		let mut cursor = field_list.walk();
		for field in field_list.named_children(&mut cursor) {
			if field.kind() != "field_declaration" {
				continue;
			}
			let Some(type_node) = field.child_by_field_name("type") else {
				continue;
			};
			if field.child_by_field_name("name").is_some() {
				self.emit_uses_type(type_node, owner, graph);
			} else {
				self.emit_extends(type_node, owner, graph);
			}
		}
	}

	fn emit_interface_body(
		&self,
		interface_node: Node<'_>,
		owner: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = interface_node.walk();
		for child in interface_node.named_children(&mut cursor) {
			match child.kind() {
				"method_elem" => {
					if let Some(params) = child.child_by_field_name("parameters") {
						let mut cur = params.walk();
						for c in params.named_children(&mut cur) {
							if let Some(t) = c.child_by_field_name("type") {
								self.emit_uses_type(t, owner, graph);
							}
						}
					}
					if let Some(result) = child.child_by_field_name("result") {
						self.emit_uses_type(result, owner, graph);
					}
				}
				"type_elem" => {
					let mut tc = child.walk();
					for t in child.named_children(&mut tc) {
						self.emit_extends(t, owner, graph);
					}
				}
				_ => {}
			}
		}
	}

	fn emit_extends(&self, type_node: Node<'_>, owner: &Moniker, graph: &mut CodeGraph) {
		match type_node.kind() {
			"type_identifier" | "qualified_type" => {
				self.emit_resolved_type_ref(
					type_node,
					owner,
					kinds::EXTENDS,
					node_position(type_node),
					graph,
				);
			}
			"pointer_type" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					self.emit_extends(c, owner, graph);
				}
			}
			"generic_type" => {
				if let Some(head) = type_node.child_by_field_name("type") {
					self.emit_extends(head, owner, graph);
				}
			}
			_ => {}
		}
	}

	fn recurse_subtree(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let walker = crate::lang::canonical_walker::CanonicalWalker::new(self, self.source_bytes);
		walker.dispatch(node, scope, graph);
	}

	pub(super) fn push_local_scope(&self) {
		self.local_scope.borrow_mut().push(HashSet::new());
	}

	pub(super) fn pop_local_scope(&self) {
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

	fn import_entry_for(&self, name: &[u8]) -> Option<ImportEntry> {
		self.imports.borrow().get(name).cloned()
	}

	fn import_confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.imports.borrow().get(name).map(|e| e.confidence)
	}

	fn resolve_type_target(&self, name: &[u8], fallback_kind: &[u8]) -> (Moniker, &'static [u8]) {
		if let Some(m) = self.type_table.get(name) {
			return (m.clone(), kinds::CONF_RESOLVED);
		}
		let target = extend_segment(&self.module, fallback_kind, name);
		let confidence = self
			.import_confidence_for(name)
			.unwrap_or(kinds::CONF_NAME_MATCH);
		(target, confidence)
	}

	fn lookup_module_callable(&self, name: &[u8], kind: &[u8]) -> Option<Moniker> {
		let seg = self
			.callable_table
			.get(&(self.module.clone(), name.to_vec()))?;
		Some(extend_segment(&self.module, kind, seg))
	}
}

pub(super) fn collect_type_table<'src>(
	root: Node<'src>,
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
			let name = node_slice(name_node, source);
			if name.is_empty() {
				continue;
			}
			let m = extend_segment(parent, kind, name);
			let attrs = DefAttrs {
				visibility: visibility_from_name(name),
				..DefAttrs::default()
			};
			let _ =
				graph.add_def_attrs(m.clone(), kind, parent, Some(node_position(tspec)), &attrs);
			out.entry(name).or_insert(m);
		}
	}
}

pub(super) fn collect_callable_table<'src>(
	root: Node<'src>,
	source: &'src [u8],
	module: &Moniker,
	type_table: &HashMap<&'src [u8], Moniker>,
	out: &mut HashMap<(Moniker, Vec<u8>), Vec<u8>>,
) {
	let mut cursor = root.walk();
	for decl in root.children(&mut cursor) {
		match decl.kind() {
			"function_declaration" => {
				let Some(name_node) = decl.child_by_field_name("name") else {
					continue;
				};
				let name = node_slice(name_node, source);
				let slots = function_param_slots(decl, source);
				let seg = callable_segment_slots(name, &slots);
				out.insert((module.clone(), name.to_vec()), seg);
			}
			"method_declaration" => {
				let Some(name_node) = decl.child_by_field_name("name") else {
					continue;
				};
				let Some(receiver) = decl.child_by_field_name("receiver") else {
					continue;
				};
				let Some(receiver_name) = receiver_type_name(receiver, source) else {
					continue;
				};
				let owner = type_table
					.get(receiver_name)
					.cloned()
					.unwrap_or_else(|| extend_segment(module, kinds::STRUCT, receiver_name));
				let name = node_slice(name_node, source);
				let slots = function_param_slots(decl, source);
				let seg = callable_segment_slots(name, &slots);
				out.insert((owner, name.to_vec()), seg);
			}
			_ => {}
		}
	}
}

fn receiver_type_name<'a>(receiver: Node<'a>, source: &'a [u8]) -> Option<&'a [u8]> {
	let mut cursor = receiver.walk();
	let param = receiver.named_children(&mut cursor).next()?;
	if param.kind() != "parameter_declaration" {
		return None;
	}
	let type_node = param.child_by_field_name("type")?;
	extract_type_name(type_node, source)
}

fn extract_type_name<'a>(node: Node<'a>, source: &'a [u8]) -> Option<&'a [u8]> {
	match node.kind() {
		"type_identifier" => Some(node_slice(node, source)),
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

fn struct_field_list<'tree>(struct_node: Node<'tree>) -> Option<Node<'tree>> {
	let mut cursor = struct_node.walk();
	struct_node
		.named_children(&mut cursor)
		.find(|&c| c.kind() == "field_declaration_list")
}

fn receiver_hint<'a>(obj: Node<'a>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SUBSCRIPT};
	match obj.kind() {
		"identifier" => node_slice(obj, source),
		"selector_expression" | "field_identifier" => HINT_MEMBER,
		"call_expression" => HINT_CALL,
		"index_expression" => HINT_SUBSCRIPT,
		_ => b"",
	}
}

fn strip_string_quotes(raw: &str) -> &str {
	let trimmed = raw
		.strip_prefix('"')
		.and_then(|s| s.strip_suffix('"'))
		.or_else(|| raw.strip_prefix('`').and_then(|s| s.strip_suffix('`')));
	trimmed.unwrap_or(raw)
}

fn stdlib_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	if STDLIB_PACKAGES.binary_search(&pieces[0]).is_ok() {
		return kinds::CONF_EXTERNAL;
	}
	kinds::CONF_IMPORTED
}

fn build_module_target(project: &[u8], pieces: &[&str]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(project);
	b.segment(kinds::EXTERNAL_PKG, pieces[0].as_bytes());
	for p in &pieces[1..] {
		b.segment(kinds::PATH, p.as_bytes());
	}
	b.build()
}

fn is_go_primitive(name: &[u8]) -> bool {
	matches!(
		name,
		b"bool"
			| b"byte" | b"complex64"
			| b"complex128"
			| b"error"
			| b"float32"
			| b"float64"
			| b"int" | b"int8"
			| b"int16"
			| b"int32"
			| b"int64"
			| b"rune" | b"string"
			| b"uint" | b"uint8"
			| b"uint16"
			| b"uint32"
			| b"uint64"
			| b"uintptr"
			| b"any"
	)
}

fn visibility_from_name(name: &[u8]) -> &'static [u8] {
	match name.first().copied() {
		Some(b) if b.is_ascii_uppercase() => kinds::VIS_PUBLIC,
		_ => kinds::VIS_MODULE,
	}
}

fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::FUNC || last.kind == kinds::METHOD
}

const STDLIB_PACKAGES: &[&str] = &[
	"archive",
	"bufio",
	"builtin",
	"bytes",
	"cmp",
	"compress",
	"container",
	"context",
	"crypto",
	"database",
	"debug",
	"embed",
	"encoding",
	"errors",
	"expvar",
	"flag",
	"fmt",
	"go",
	"hash",
	"html",
	"image",
	"index",
	"io",
	"iter",
	"log",
	"maps",
	"math",
	"mime",
	"net",
	"os",
	"path",
	"plugin",
	"reflect",
	"regexp",
	"runtime",
	"slices",
	"sort",
	"strconv",
	"strings",
	"sync",
	"syscall",
	"testing",
	"text",
	"time",
	"unicode",
	"unsafe",
];

fn spec_children<'tree>(node: Node<'tree>, spec_kind: &str) -> Vec<Node<'tree>> {
	let mut out = Vec::new();
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		match child.kind() {
			k if k == spec_kind => out.push(child),
			"var_spec_list" | "const_spec_list" => {
				let mut nc = child.walk();
				for spec in child.named_children(&mut nc) {
					if spec.kind() == spec_kind {
						out.push(spec);
					}
				}
			}
			_ => {}
		}
	}
	out
}
