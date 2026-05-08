
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs};
use crate::core::moniker::Moniker;

use super::canonicalize::{extend_callable_typed, extend_segment, node_position};
use super::kinds;
use super::scope::{is_callable_scope, is_class_scope, section_title, visibility_from_name};

pub(super) struct Walker<'src> {
	pub(super) source_bytes: &'src [u8],
	pub(super) module: Moniker,
	pub(super) deep: bool,
	pub(super) local_scope: RefCell<Vec<HashSet<&'src [u8]>>>,
	pub(super) imports: RefCell<HashMap<&'src [u8], &'static [u8]>>,
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
			"comment" => self.handle_comment(node, scope, graph),
			"import_statement" => self.handle_import(node, scope, graph),
			"import_from_statement" => self.handle_import_from(node, scope, graph),
			"decorated_definition" => self.handle_decorated(node, scope, graph),
			"class_definition" => self.handle_class(node, scope, graph, &[]),
			"function_definition" => self.handle_function(node, scope, graph, &[]),
			"call" => self.handle_call(node, scope, graph),
			"assignment" => self.handle_assignment(node, scope, graph),
			"identifier" => self.handle_identifier(node, scope, graph),
			"lambda" => self.handle_lambda(node, scope, graph),
			"for_statement" => self.handle_for(node, scope, graph),
			"subscript" => {
				self.walk(node, scope, graph);
			}
			_ => self.walk(node, scope, graph),
		}
	}


	fn handle_comment(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let text = self.text_of(node);
		let Some(title) = section_title(text) else { return };
		let m = extend_segment(scope, kinds::SECTION, title.as_bytes());
		let _ = graph.add_def(m, kinds::SECTION, scope, Some(node_position(node)));
	}


	fn handle_decorated(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut decorators: Vec<Node<'_>> = Vec::new();
		let mut def_node: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"decorator" => decorators.push(c),
				"class_definition" | "function_definition" => def_node = Some(c),
				_ => {}
			}
		}
		let Some(def) = def_node else {
			self.walk(node, scope, graph);
			return;
		};
		match def.kind() {
			"class_definition" => self.handle_class(def, scope, graph, &decorators),
			"function_definition" => self.handle_function(def, scope, graph, &decorators),
			_ => self.walk(node, scope, graph),
		}
	}


	fn handle_class(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		decorators: &[Node<'_>],
	) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_segment(scope, kinds::CLASS, name.as_bytes());
		let attrs = DefAttrs {
			visibility: visibility_from_name(name.as_bytes()),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::CLASS,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		if let Some(supers) = node.child_by_field_name("superclasses") {
			self.emit_base_classes(supers, &m, graph);
		}
		for d in decorators {
			self.handle_decorator(*d, &m, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
	}

	fn emit_base_classes(&self, supers: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = supers.walk();
		for child in supers.named_children(&mut cursor) {
			let name = match child.kind() {
				"identifier" => self.text_of(child).to_string(),
				"attribute" => last_attribute(child, self.source_bytes).to_string(),
				"subscript" => match child.child_by_field_name("value") {
					Some(v) => match v.kind() {
						"identifier" => self.text_of(v).to_string(),
						"attribute" => last_attribute(v, self.source_bytes).to_string(),
						_ => continue,
					},
					None => continue,
				},
				"keyword_argument" => continue,
				_ => continue,
			};
			if name.is_empty() {
				continue;
			}
			let (target, confidence) = self.resolve_type_target(name.as_bytes(), kinds::CLASS);
			let attrs = crate::core::code_graph::RefAttrs {
				confidence,
				..crate::core::code_graph::RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				parent,
				target,
				kinds::EXTENDS,
				Some(node_position(child)),
				&attrs,
			);
		}
	}


	fn handle_function(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		decorators: &[Node<'_>],
	) {
		let Some(name) = self.field_text(node, "name") else { return };
		let is_method = is_class_scope(scope);
		let kind = if is_method { kinds::METHOD } else { kinds::FUNCTION };

		let param_types = collect_param_types(node, self.source_bytes, is_method);
		let signature = crate::lang::callable::join_bytes_with_comma(&param_types);
		let m = extend_callable_typed(scope, kind, name.as_bytes(), &param_types);
		let attrs = DefAttrs {
			visibility: visibility_from_name(name.as_bytes()),
			signature: signature.as_slice(),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kind,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		for d in decorators {
			self.handle_decorator(*d, &m, graph);
		}

		if let Some(rt) = node.child_by_field_name("return_type") {
			self.emit_uses_type(rt, &m, graph);
		}

		self.push_local_scope();

		if let Some(params) = node.child_by_field_name("parameters") {
			self.handle_parameters(params, &m, graph);
		}

		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}

		self.pop_local_scope();
	}

	fn handle_parameters(
		&self,
		params: Node<'_>,
		callable: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			let (name_node, type_node) = parameter_name_and_type(child);
			let Some(name_node) = name_node else { continue };
			let name_str = self.text_of(name_node);
			if name_str.is_empty() {
				continue;
			}
			self.record_local(name_str.as_bytes());
			if self.deep {
				let m = extend_segment(callable, kinds::PARAM, name_str.as_bytes());
				let _ = graph.add_def(
					m,
					kinds::PARAM,
					callable,
					Some(node_position(child)),
				);
			}
			if let Some(t) = type_node {
				self.emit_uses_type(t, callable, graph);
			}
		}
	}


	fn handle_assignment(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}

		let inside_callable = is_callable_scope(scope, &self.module);
		if inside_callable {
			if let Some(left) = node.child_by_field_name("left") {
				self.record_local_pattern(left);
				if self.deep {
					self.emit_local_pattern(left, scope, graph);
				}
			}
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.dispatch(right, scope, graph);
		}
	}

	fn record_local_pattern(&self, node: Node<'_>) {
		match node.kind() {
			"identifier" => {
				let name = self.text_of(node);
				if !name.is_empty() {
					self.record_local(name.as_bytes());
				}
			}
			"pattern_list" | "tuple_pattern" | "list_pattern" | "list_splat_pattern" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					self.record_local_pattern(c);
				}
			}
			_ => {}
		}
	}

	fn emit_local_pattern(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		match node.kind() {
			"identifier" => {
				let name = self.text_of(node);
				if !name.is_empty() {
					let m = extend_segment(scope, kinds::LOCAL, name.as_bytes());
					let _ = graph.add_def(
						m,
						kinds::LOCAL,
						scope,
						Some(node_position(node)),
					);
				}
			}
			"pattern_list" | "tuple_pattern" | "list_pattern" | "list_splat_pattern" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					self.emit_local_pattern(c, scope, graph);
				}
			}
			_ => {}
		}
	}


	fn handle_for(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if is_callable_scope(scope, &self.module) {
			if let Some(left) = node.child_by_field_name("left") {
				self.record_local_pattern(left);
				if self.deep {
					self.emit_local_pattern(left, scope, graph);
				}
			}
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.dispatch(right, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, scope, graph);
		}
	}

	fn handle_lambda(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			let mut cursor = params.walk();
			for child in params.named_children(&mut cursor) {
				let (name_node, _ty) = parameter_name_and_type(child);
				let Some(nn) = name_node else { continue };
				let name = self.text_of(nn);
				if !name.is_empty() {
					self.record_local(name.as_bytes());
					if self.deep && is_callable_scope(scope, &self.module) {
						let m = extend_segment(scope, kinds::PARAM, name.as_bytes());
						let _ = graph.add_def(
							m,
							kinds::PARAM,
							scope,
							Some(node_position(nn)),
						);
					}
				}
			}
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.dispatch(body, scope, graph);
		}
		self.pop_local_scope();
	}


	fn handle_decorator(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for c in node.named_children(&mut cursor) {
			let (name, name_node) = match c.kind() {
				"identifier" => (self.text_of(c).to_string(), c),
				"attribute" => (last_attribute(c, self.source_bytes).to_string(), c),
				"call" => match c.child_by_field_name("function") {
					Some(f) => match f.kind() {
						"identifier" => (self.text_of(f).to_string(), f),
						"attribute" => (last_attribute(f, self.source_bytes).to_string(), f),
						_ => {
							self.walk(c, parent, graph);
							continue;
						}
					},
					None => continue,
				},
				_ => continue,
			};
			if name.is_empty() {
				continue;
			}
			let (target, confidence) = self.resolve_type_target(name.as_bytes(), kinds::CLASS);
			let attrs = crate::core::code_graph::RefAttrs {
				confidence,
				..crate::core::code_graph::RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				parent,
				target,
				kinds::ANNOTATES,
				Some(node_position(name_node)),
				&attrs,
			);
			if c.kind() == "call" {
				if let Some(args) = c.child_by_field_name("arguments") {
					self.walk(args, parent, graph);
				}
			}
		}
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

pub(super) fn collect_type_table<'src>(
	node: Node<'_>,
	source: &'src [u8],
	parent: &Moniker,
	out: &mut HashMap<&'src [u8], Moniker>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		let class_node = match child.kind() {
			"class_definition" => Some(child),
			"decorated_definition" => child.child_by_field_name("definition").and_then(|d| {
				if d.kind() == "class_definition" {
					Some(d)
				} else {
					None
				}
			}),
			_ => None,
		};
		let Some(class_node) = class_node else {
			collect_type_table(child, source, parent, out);
			continue;
		};
		let Some(name_node) = class_node.child_by_field_name("name") else { continue };
		let Ok(name) = name_node.utf8_text(source) else { continue };
		let m = super::canonicalize::extend_segment(parent, kinds::CLASS, name.as_bytes());
		out.entry(name.as_bytes()).or_insert_with(|| m.clone());
		if let Some(body) = class_node.child_by_field_name("body") {
			collect_type_table(body, source, &m, out);
		}
	}
}

pub(super) fn collect_param_types(
	function: Node<'_>,
	source: &[u8],
	is_method: bool,
) -> Vec<Vec<u8>> {
	let Some(params) = function.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut types: Vec<Vec<u8>> = Vec::new();
	let mut cursor = params.walk();
	let mut idx = 0usize;
	for child in params.named_children(&mut cursor) {
		let (name_node, type_node) = parameter_name_and_type(child);
		let Some(name_node) = name_node else { continue };
		let Ok(name_str) = name_node.utf8_text(source) else { continue };
		if is_method && idx == 0 && (name_str == "self" || name_str == "cls") {
			idx += 1;
			continue;
		}
		idx += 1;
		let ty = type_node
			.and_then(|t| t.utf8_text(source).ok())
			.map(|s| s.trim().as_bytes().to_vec())
			.unwrap_or_else(|| b"_".to_vec());
		types.push(ty);
	}
	types
}

fn parameter_name_and_type<'tree>(
	param: Node<'tree>,
) -> (Option<Node<'tree>>, Option<Node<'tree>>) {
	match param.kind() {
		"identifier" => (Some(param), None),
		"default_parameter" => {
			let n = param.child_by_field_name("name");
			(n, None)
		}
		"typed_parameter" => {
			let ty = param.child_by_field_name("type");
			let mut cursor = param.walk();
			let mut name = None;
			for c in param.named_children(&mut cursor) {
				if matches!(
					c.kind(),
					"identifier" | "list_splat_pattern" | "dictionary_splat_pattern"
				) {
					name = Some(c);
					break;
				}
			}
			(name, ty)
		}
		"typed_default_parameter" => {
			let n = param.child_by_field_name("name");
			let t = param.child_by_field_name("type");
			(n, t)
		}
		"list_splat_pattern" | "dictionary_splat_pattern" => {
			let mut cursor = param.walk();
			let mut name = None;
			for c in param.named_children(&mut cursor) {
				if c.kind() == "identifier" {
					name = Some(c);
					break;
				}
			}
			(name, None)
		}
		_ => (None, None),
	}
}

pub(super) fn last_attribute<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
	if let Some(attr) = node.child_by_field_name("attribute") {
		return attr.utf8_text(source).unwrap_or("");
	}
	""
}
