use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{
	CallableSlot, callable_segment_slots, extend_callable_slots, extend_segment,
	join_bytes_with_comma, slot_signature_bytes,
};
use crate::lang::strategy::{LangStrategy, NodeShape, RefSpec, Symbol};
use crate::lang::tree_util::{node_position, node_slice};

use super::kinds;

pub(super) struct Strategy<'src> {
	pub(super) module: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) imports: RefCell<HashMap<Vec<u8>, &'static [u8]>>,
	pub(super) import_targets: RefCell<HashMap<Vec<u8>, Moniker>>,
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
			"comment" => NodeShape::Annotation {
				kind: kinds::COMMENT,
			},
			"import_statement" => {
				self.handle_import(node, scope, graph);
				NodeShape::Skip
			}
			"import_from_statement" => {
				self.handle_import_from(node, scope, graph);
				NodeShape::Skip
			}
			"decorated_definition" => self.classify_decorated(node, scope, source, graph),
			"class_definition" => self.classify_class(node, scope, source, graph, &[]),
			"function_definition" => self.classify_function(node, scope, source, graph, &[]),
			"call" => {
				self.handle_call(node, scope, graph);
				NodeShape::Skip
			}
			"assignment" => {
				self.handle_assignment(node, scope, graph);
				NodeShape::Skip
			}
			"identifier" => {
				self.handle_identifier(node, scope, graph);
				NodeShape::Skip
			}
			"lambda" => {
				self.handle_lambda(node, scope, graph);
				NodeShape::Skip
			}
			"for_statement" => {
				self.handle_for(node, scope, graph);
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
		if kind != kinds::FUNCTION && kind != kinds::METHOD {
			return;
		}
		if let Some(rt) = node.child_by_field_name("return_type") {
			self.emit_uses_type(rt, moniker, graph);
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			self.emit_param_defs_and_types(params, moniker, source, graph);
		}
	}

	fn after_body(&self, kind: &[u8], _moniker: &Moniker) {
		if kind == kinds::FUNCTION || kind == kinds::METHOD {
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
		if sym_kind != kinds::FUNCTION && sym_kind != kinds::METHOD && sym_kind != kinds::CLASS {
			return;
		}
		let Some(body) = node.child_by_field_name("body") else {
			return;
		};
		if let Some(docstring) = first_docstring(body) {
			emit_docstring_def(docstring, sym_moniker, graph);
		}
	}
}

impl<'src_lang> Strategy<'src_lang> {
	fn classify_decorated<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
	) -> NodeShape<'src> {
		let mut decorators: Vec<Node<'src>> = Vec::new();
		let mut def_node: Option<Node<'src>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"decorator" => decorators.push(c),
				"class_definition" | "function_definition" => def_node = Some(c),
				_ => {}
			}
		}
		let Some(def) = def_node else {
			return NodeShape::Recurse;
		};
		match def.kind() {
			"class_definition" => self.classify_class(def, scope, source, graph, &decorators),
			"function_definition" => self.classify_function(def, scope, source, graph, &decorators),
			_ => NodeShape::Recurse,
		}
	}

	fn classify_class<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		_graph: &mut CodeGraph,
		decorators: &[Node<'src>],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::CLASS, name);

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		if let Some(supers) = node.child_by_field_name("superclasses") {
			self.collect_base_class_refs(supers, &mut annotated_by);
		}
		for d in decorators {
			self.collect_decorator_refs(*d, &mut annotated_by);
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::CLASS,
			visibility: visibility_from_name(name),
			signature: None,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_function<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CodeGraph,
		decorators: &[Node<'src>],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let is_method = is_class_scope(scope);
		let kind = if is_method {
			kinds::METHOD
		} else {
			kinds::FUNCTION
		};

		let slots = collect_param_slots(node, source, is_method);
		let signature =
			join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
		let moniker = extend_callable_slots(scope, kind, name, &slots);

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		for d in decorators {
			self.collect_decorator_refs(*d, &mut annotated_by);
		}

		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			self.record_param_locals(params, source);
		}
		let _ = graph;

		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility: visibility_from_name(name),
			signature: Some(signature),
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn record_param_locals(&self, params: Node<'_>, source: &[u8]) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			let (name_node, _type_node) = parameter_name_and_type(child);
			let Some(name_node) = name_node else { continue };
			let name = node_slice(name_node, source);
			if name.is_empty() {
				continue;
			}
			self.record_local(name);
		}
	}

	fn emit_param_defs_and_types(
		&self,
		params: Node<'_>,
		callable: &Moniker,
		source: &[u8],
		graph: &mut CodeGraph,
	) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			let (name_node, type_node) = parameter_name_and_type(child);
			let Some(name_node) = name_node else { continue };
			let name = node_slice(name_node, source);
			if name.is_empty() {
				continue;
			}
			if self.deep {
				let m = extend_segment(callable, kinds::PARAM, name);
				let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(child)));
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
		if inside_callable && let Some(left) = node.child_by_field_name("left") {
			self.record_local_pattern(left);
			if self.deep {
				self.emit_local_pattern(left, scope, graph);
			}
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.recurse_subtree(right, scope, graph);
		}
	}

	fn handle_for(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if is_callable_scope(scope, &self.module)
			&& let Some(left) = node.child_by_field_name("left")
		{
			self.record_local_pattern(left);
			if self.deep {
				self.emit_local_pattern(left, scope, graph);
			}
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.recurse_subtree(right, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.recurse_subtree(body, scope, graph);
		}
	}

	fn handle_lambda(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			let mut cursor = params.walk();
			for child in params.named_children(&mut cursor) {
				let (name_node, _ty) = parameter_name_and_type(child);
				let Some(nn) = name_node else { continue };
				let name = node_slice(nn, self.source_bytes);
				if name.is_empty() {
					continue;
				}
				self.record_local(name);
				if self.deep && is_callable_scope(scope, &self.module) {
					let m = extend_segment(scope, kinds::PARAM, name);
					let _ = graph.add_def(m, kinds::PARAM, scope, Some(node_position(nn)));
				}
			}
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.recurse_subtree(body, scope, graph);
		}
		self.pop_local_scope();
	}

	fn handle_call(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let Some(callee) = node.child_by_field_name("function") else {
			self.recurse_subtree(node, scope, graph);
			return;
		};

		match callee.kind() {
			"identifier" => {
				let name = node_slice(callee, self.source_bytes);
				if !name.is_empty() {
					let confidence = match self.import_confidence_for(name) {
						Some(c) => Some(c),
						None => self.name_confidence(name),
					};
					if let Some(confidence) = confidence {
						let target = if confidence == kinds::CONF_LOCAL {
							extend_segment(scope, kinds::LOCAL, name)
						} else if let Some(m) = self.lookup_import_target(name) {
							m
						} else {
							self.lookup_callable_in_scope(scope, name, kinds::METHOD)
								.or_else(|| {
									self.lookup_callable_in_scope(scope, name, kinds::FUNCTION)
								})
								.unwrap_or_else(|| {
									extend_segment(&self.module, kinds::FUNCTION, name)
								})
						};
						let attrs = RefAttrs {
							confidence,
							..RefAttrs::default()
						};
						let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
					}
				}
			}
			"attribute" => {
				let name = last_attribute(callee, self.source_bytes);
				if !name.is_empty() {
					let receiver = callee.child_by_field_name("object");
					let hint = receiver
						.map(|r| receiver_hint(r, self.source_bytes))
						.unwrap_or(b"");
					let target = if matches!(hint, b"self" | b"cls") {
						self.lookup_callable_in_scope(scope, name.as_bytes(), kinds::METHOD)
							.unwrap_or_else(|| {
								extend_segment(&self.module, kinds::METHOD, name.as_bytes())
							})
					} else {
						extend_segment(&self.module, kinds::METHOD, name.as_bytes())
					};
					let attrs = RefAttrs {
						receiver_hint: hint,
						confidence: kinds::CONF_NAME_MATCH,
						..RefAttrs::default()
					};
					let _ =
						graph.add_ref_attrs(scope, target, kinds::METHOD_CALL, Some(pos), &attrs);
				}
				if let Some(obj) = callee.child_by_field_name("object") {
					self.recurse_subtree(obj, scope, graph);
				}
			}
			_ => self.recurse_subtree(callee, scope, graph),
		}

		if let Some(args) = node.child_by_field_name("arguments") {
			self.recurse_subtree(args, scope, graph);
		}
	}

	fn handle_identifier(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let name = node_slice(node, self.source_bytes);
		if name.is_empty() {
			return;
		}
		let confidence = match self.import_confidence_for(name) {
			Some(c) => Some(c),
			None => self.name_confidence(name),
		};
		let Some(confidence) = confidence else {
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

	fn handle_import(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let mut cursor = node.walk();
		let mut targets: Vec<Node<'_>> = Vec::new();
		for c in node.children(&mut cursor) {
			if matches!(c.kind(), "dotted_name" | "aliased_import") {
				targets.push(c);
			}
		}
		for t in targets {
			self.emit_import_module(t, scope, graph, pos);
		}
	}

	fn emit_import_module(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		pos: (u32, u32),
	) {
		let (path_node, alias) = match node.kind() {
			"aliased_import" => (
				node.child_by_field_name("name"),
				node.child_by_field_name("alias")
					.and_then(|n| n.utf8_text(self.source_bytes).ok())
					.unwrap_or(""),
			),
			_ => (Some(node), ""),
		};
		let Some(path_node) = path_node else { return };
		let pieces = dotted_pieces(path_node, self.source_bytes);
		if pieces.is_empty() {
			return;
		}
		let confidence = external_or_imported(&pieces);
		let bind = if !alias.is_empty() { alias } else { pieces[0] };
		self.bind_import(bind.as_bytes(), confidence);

		let target = build_module_target(&self.module, &pieces, 0, confidence);
		self.bind_import_target(bind.as_bytes(), &target);
		let attrs = RefAttrs {
			confidence,
			alias: alias.as_bytes(),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
	}

	fn handle_import_from(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let Some(module_node) = node.child_by_field_name("module_name") else {
			return;
		};
		let (pieces, leading_dots) = match module_node.kind() {
			"relative_import" => relative_import_pieces(module_node, self.source_bytes),
			"dotted_name" => (dotted_pieces(module_node, self.source_bytes), 0),
			_ => return,
		};

		let mut wildcard = false;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "wildcard_import" {
				wildcard = true;
			}
		}
		let confidence = if leading_dots > 0 {
			kinds::CONF_IMPORTED
		} else {
			external_or_imported(&pieces)
		};
		let module_target = build_module_target(&self.module, &pieces, leading_dots, confidence);

		if wildcard {
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				scope,
				module_target,
				kinds::IMPORTS_MODULE,
				Some(pos),
				&attrs,
			);
			return;
		}

		let names = collect_from_import_names(node, self.source_bytes);
		for (name, alias) in names {
			let bind = if !alias.is_empty() { alias } else { name };
			self.bind_import(bind.as_bytes(), confidence);
			let target = build_imported_symbol_target(
				&self.module,
				&pieces,
				leading_dots,
				name.as_bytes(),
				confidence,
			);
			self.bind_import_target(bind.as_bytes(), &target);
			let attrs = RefAttrs {
				confidence,
				alias: alias.as_bytes(),
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_SYMBOL, Some(pos), &attrs);
		}
	}

	fn collect_base_class_refs(&self, supers: Node<'_>, out: &mut Vec<RefSpec>) {
		let mut cursor = supers.walk();
		for child in supers.named_children(&mut cursor) {
			let name = match child.kind() {
				"identifier" => node_slice(child, self.source_bytes).to_vec(),
				"attribute" => last_attribute(child, self.source_bytes).as_bytes().to_vec(),
				"subscript" => match child.child_by_field_name("value") {
					Some(v) => match v.kind() {
						"identifier" => node_slice(v, self.source_bytes).to_vec(),
						"attribute" => last_attribute(v, self.source_bytes).as_bytes().to_vec(),
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
			let (target, confidence) = self.resolve_type_target(&name, kinds::CLASS);
			out.push(RefSpec {
				kind: kinds::EXTENDS,
				target,
				confidence,
				position: node_position(child),
				receiver_hint: b"",
				alias: b"",
			});
		}
	}

	fn collect_decorator_refs(&self, node: Node<'_>, out: &mut Vec<RefSpec>) {
		let mut cursor = node.walk();
		for c in node.named_children(&mut cursor) {
			let (name, name_node) = match c.kind() {
				"identifier" => (node_slice(c, self.source_bytes).to_vec(), c),
				"attribute" => (last_attribute(c, self.source_bytes).as_bytes().to_vec(), c),
				"call" => match c.child_by_field_name("function") {
					Some(f) => match f.kind() {
						"identifier" => (node_slice(f, self.source_bytes).to_vec(), f),
						"attribute" => {
							(last_attribute(f, self.source_bytes).as_bytes().to_vec(), f)
						}
						_ => continue,
					},
					None => continue,
				},
				_ => continue,
			};
			if name.is_empty() {
				continue;
			}
			let (target, confidence) = self.resolve_type_target(&name, kinds::CLASS);
			out.push(RefSpec {
				kind: kinds::ANNOTATES,
				target,
				confidence,
				position: node_position(name_node),
				receiver_hint: b"",
				alias: b"",
			});
		}
	}

	fn emit_uses_type(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		match node.kind() {
			"type" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					self.emit_uses_type(c, scope, graph);
				}
			}
			"identifier" => {
				let name = node_slice(node, self.source_bytes);
				if name.is_empty() {
					return;
				}
				let (target, confidence) = self.resolve_type_target(name, kinds::CLASS);
				let attrs = RefAttrs {
					confidence,
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
			"attribute" => {
				let name = last_attribute(node, self.source_bytes);
				if name.is_empty() {
					return;
				}
				let (target, confidence) = self.resolve_type_target(name.as_bytes(), kinds::CLASS);
				let attrs = RefAttrs {
					confidence,
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
			"subscript" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					if c.kind() != "slice" {
						self.emit_uses_type(c, scope, graph);
					}
				}
			}
			"generic_type" | "type_parameter" | "member_type" | "constrained_type"
			| "splat_type" | "tuple" | "list" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					self.emit_uses_type(c, scope, graph);
				}
			}
			_ => {}
		}
	}

	fn record_local_pattern(&self, node: Node<'_>) {
		match node.kind() {
			"identifier" => {
				let name = node_slice(node, self.source_bytes);
				if !name.is_empty() {
					self.record_local_static(name);
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
				let name = node_slice(node, self.source_bytes);
				if !name.is_empty() {
					let m = extend_segment(scope, kinds::LOCAL, name);
					let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(node)));
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

	fn record_local_static(&self, name: &[u8]) {
		self.record_local(name);
	}

	fn bind_import(&self, name: &[u8], confidence: &'static [u8]) {
		self.imports.borrow_mut().insert(name.to_vec(), confidence);
	}

	fn bind_import_target(&self, name: &[u8], target: &Moniker) {
		if name.is_empty() {
			return;
		}
		self.import_targets
			.borrow_mut()
			.insert(name.to_vec(), target.clone());
	}

	fn lookup_import_target(&self, name: &[u8]) -> Option<Moniker> {
		self.import_targets.borrow().get(name).cloned()
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

	fn import_confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.imports.borrow().get(name).copied()
	}

	fn resolve_type_target(&self, name: &[u8], fallback_kind: &[u8]) -> (Moniker, &'static [u8]) {
		if let Some(m) = self.type_table.get(name) {
			return (m.clone(), kinds::CONF_RESOLVED);
		}
		if let Some(m) = self.lookup_import_target(name) {
			let confidence = self
				.import_confidence_for(name)
				.unwrap_or(kinds::CONF_NAME_MATCH);
			return (m, confidence);
		}
		let target = extend_segment(&self.module, fallback_kind, name);
		let confidence = self
			.import_confidence_for(name)
			.unwrap_or(kinds::CONF_NAME_MATCH);
		(target, confidence)
	}

	fn lookup_callable_in_scope(
		&self,
		scope: &Moniker,
		name: &[u8],
		kind: &[u8],
	) -> Option<Moniker> {
		let parent = enclosing_class(scope, &self.module).unwrap_or_else(|| self.module.clone());
		let seg = self.callable_table.get(&(parent.clone(), name.to_vec()))?;
		Some(extend_segment(&parent, kind, seg))
	}
}

fn enclosing_class(scope: &Moniker, module: &Moniker) -> Option<Moniker> {
	let view = scope.as_view();
	let segs: Vec<_> = view.segments().collect();
	let idx = segs.iter().rposition(|s| s.kind == b"class")?;
	let mut b = crate::core::moniker::MonikerBuilder::new();
	b.project(view.project());
	for s in &segs[..=idx] {
		b.segment(s.kind, s.name);
	}
	let out = b.build();
	if &out == module { None } else { Some(out) }
}

pub(super) fn collect_callable_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	is_class_scope: bool,
	out: &mut HashMap<(Moniker, Vec<u8>), Vec<u8>>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		let (class_node, function_node) = match child.kind() {
			"class_definition" => (Some(child), None),
			"function_definition" => (None, Some(child)),
			"decorated_definition" => {
				let d = child.child_by_field_name("definition");
				match d.map(|n| n.kind()) {
					Some("class_definition") => (d, None),
					Some("function_definition") => (None, d),
					_ => (None, None),
				}
			}
			_ => (None, None),
		};
		if let Some(class_node) = class_node {
			let Some(name_node) = class_node.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, source);
			let scope = extend_segment(parent, kinds::CLASS, name);
			if let Some(body) = class_node.child_by_field_name("body") {
				collect_callable_table(body, source, &scope, true, out);
			}
		} else if let Some(function_node) = function_node {
			let Some(name_node) = function_node.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, source);
			let slots = collect_param_slots(function_node, source, is_class_scope);
			let seg = callable_segment_slots(name, &slots);
			out.insert((parent.clone(), name.to_vec()), seg);
		} else {
			collect_callable_table(child, source, parent, is_class_scope, out);
		}
	}
}

pub(super) fn collect_type_table<'src>(
	node: Node<'src>,
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
		let Some(name_node) = class_node.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, source);
		let m = extend_segment(parent, kinds::CLASS, name);
		out.entry(name).or_insert_with(|| m.clone());
		if let Some(body) = class_node.child_by_field_name("body") {
			collect_type_table(body, source, &m, out);
		}
	}
}

fn parameter_name_and_type<'tree>(
	param: Node<'tree>,
) -> (Option<Node<'tree>>, Option<Node<'tree>>) {
	match param.kind() {
		"identifier" => (Some(param), None),
		"default_parameter" => (param.child_by_field_name("name"), None),
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
		"typed_default_parameter" => (
			param.child_by_field_name("name"),
			param.child_by_field_name("type"),
		),
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

fn collect_param_slots(function: Node<'_>, source: &[u8], is_method: bool) -> Vec<CallableSlot> {
	let Some(params) = function.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut out: Vec<CallableSlot> = Vec::new();
	let mut cursor = params.walk();
	let mut idx = 0usize;
	for child in params.named_children(&mut cursor) {
		let (name_node, type_node) = parameter_name_and_type(child);
		let Some(name_node) = name_node else { continue };
		let Ok(name_str) = name_node.utf8_text(source) else {
			continue;
		};
		if is_method && idx == 0 && (name_str == "self" || name_str == "cls") {
			idx += 1;
			continue;
		}
		idx += 1;
		let r#type = type_node
			.and_then(|t| t.utf8_text(source).ok())
			.map(crate::lang::callable::normalize_type_text)
			.unwrap_or_default();
		out.push(CallableSlot {
			name: name_str.as_bytes().to_vec(),
			r#type,
		});
	}
	out
}

fn last_attribute<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
	if let Some(attr) = node.child_by_field_name("attribute") {
		return attr.utf8_text(source).unwrap_or("");
	}
	""
}

fn receiver_hint<'a>(obj: Node<'_>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_CLS, HINT_MEMBER, HINT_SELF, HINT_SUBSCRIPT};
	match obj.kind() {
		"identifier" => match obj.utf8_text(source).unwrap_or("") {
			"self" => HINT_SELF,
			"cls" => HINT_CLS,
			other => other.as_bytes(),
		},
		"attribute" => HINT_MEMBER,
		"call" => HINT_CALL,
		"subscript" => HINT_SUBSCRIPT,
		_ => b"",
	}
}

fn dotted_pieces<'a>(node: Node<'_>, source: &'a [u8]) -> Vec<&'a str> {
	let mut out = Vec::new();
	let mut cursor = node.walk();
	for c in node.named_children(&mut cursor) {
		if c.kind() == "identifier"
			&& let Ok(s) = c.utf8_text(source)
		{
			out.push(s);
		}
	}
	out
}

fn relative_import_pieces<'a>(node: Node<'_>, source: &'a [u8]) -> (Vec<&'a str>, usize) {
	let mut leading_dots = 0usize;
	let mut pieces: Vec<&str> = Vec::new();
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		match c.kind() {
			"import_prefix" => {
				if let Ok(s) = c.utf8_text(source) {
					leading_dots = s.chars().filter(|ch| *ch == '.').count();
				}
			}
			"dotted_name" => {
				pieces = dotted_pieces(c, source);
			}
			_ => {}
		}
	}
	(pieces, leading_dots)
}

fn collect_from_import_names<'src>(
	node: Node<'_>,
	source: &'src [u8],
) -> Vec<(&'src str, &'src str)> {
	let mut out: Vec<(&'src str, &'src str)> = Vec::new();
	let mut cursor = node.walk();
	for c in node.children_by_field_name("name", &mut cursor) {
		match c.kind() {
			"dotted_name" => {
				let leaf = dotted_leaf(c, source);
				if !leaf.is_empty() {
					out.push((leaf, ""));
				}
			}
			"aliased_import" => {
				let name_node = c.child_by_field_name("name");
				let alias = c
					.child_by_field_name("alias")
					.and_then(|n| n.utf8_text(source).ok())
					.unwrap_or("");
				let leaf = match name_node {
					Some(n) if n.kind() == "dotted_name" => dotted_leaf(n, source),
					Some(n) => n.utf8_text(source).unwrap_or(""),
					None => "",
				};
				if !leaf.is_empty() {
					out.push((leaf, alias));
				}
			}
			_ => {}
		}
	}
	out
}

fn dotted_leaf<'src>(node: Node<'_>, source: &'src [u8]) -> &'src str {
	let mut cursor = node.walk();
	let mut last = "";
	for c in node.named_children(&mut cursor) {
		if c.kind() == "identifier"
			&& let Ok(s) = c.utf8_text(source)
		{
			last = s;
		}
	}
	last
}

fn build_module_target(
	importer: &Moniker,
	pieces: &[&str],
	leading_dots: usize,
	confidence: &[u8],
) -> Moniker {
	let project = importer.as_view().project();
	if leading_dots > 0 {
		return build_relative_module_target(importer, pieces, leading_dots);
	}
	if pieces.is_empty() {
		let mut b = MonikerBuilder::new();
		b.project(project);
		return b.build();
	}
	if confidence == kinds::CONF_IMPORTED {
		let mut b = MonikerBuilder::new();
		b.project(project);
		b.segment(crate::lang::kinds::LANG, b"python");
		let last = pieces.len() - 1;
		for (i, p) in pieces.iter().enumerate() {
			let kind = if i == last {
				kinds::MODULE
			} else {
				kinds::PACKAGE
			};
			b.segment(kind, p.as_bytes());
		}
		return b.build();
	}
	let mut b = MonikerBuilder::new();
	b.project(project);
	b.segment(kinds::EXTERNAL_PKG, pieces[0].as_bytes());
	for p in &pieces[1..] {
		b.segment(kinds::PATH, p.as_bytes());
	}
	b.build()
}

fn build_relative_module_target(
	importer: &Moniker,
	pieces: &[&str],
	leading_dots: usize,
) -> Moniker {
	let view = importer.as_view();
	let depth = view.segment_count() as usize;
	let keep = depth
		.saturating_sub(1)
		.saturating_sub(leading_dots.saturating_sub(1));
	if keep == 0 {
		let mut b = MonikerBuilder::new();
		b.project(view.project());
		let head = ".".repeat(leading_dots);
		b.segment(kinds::EXTERNAL_PKG, head.as_bytes());
		for p in pieces {
			b.segment(kinds::PATH, p.as_bytes());
		}
		return b.build();
	}
	let mut b = MonikerBuilder::from_view(view);
	b.truncate(keep);
	if pieces.is_empty() {
		return b.build();
	}
	let last = pieces.len() - 1;
	for (i, p) in pieces.iter().enumerate() {
		let kind = if i == last {
			kinds::MODULE
		} else {
			kinds::PACKAGE
		};
		b.segment(kind, p.as_bytes());
	}
	b.build()
}

fn build_imported_symbol_target(
	importer: &Moniker,
	pieces: &[&str],
	leading_dots: usize,
	name: &[u8],
	confidence: &[u8],
) -> Moniker {
	let module = build_module_target(importer, pieces, leading_dots, confidence);
	let language_regime =
		leading_dots > 0 || (confidence == kinds::CONF_IMPORTED && !pieces.is_empty());
	if language_regime {
		extend_segment(&module, kinds::PATH, name)
	} else {
		extend_segment(&module, kinds::FUNCTION, name)
	}
}

fn external_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	if STDLIB_PACKAGES.binary_search(&pieces[0]).is_ok() {
		return kinds::CONF_EXTERNAL;
	}
	kinds::CONF_IMPORTED
}

const STDLIB_PACKAGES: &[&str] = &[
	"abc",
	"argparse",
	"ast",
	"asyncio",
	"base64",
	"collections",
	"concurrent",
	"contextlib",
	"copy",
	"csv",
	"dataclasses",
	"datetime",
	"decimal",
	"difflib",
	"enum",
	"errno",
	"functools",
	"gc",
	"glob",
	"hashlib",
	"heapq",
	"http",
	"importlib",
	"inspect",
	"io",
	"ipaddress",
	"itertools",
	"json",
	"logging",
	"math",
	"multiprocessing",
	"operator",
	"os",
	"pathlib",
	"pickle",
	"pkgutil",
	"platform",
	"pprint",
	"queue",
	"random",
	"re",
	"secrets",
	"shutil",
	"signal",
	"socket",
	"sqlite3",
	"ssl",
	"stat",
	"string",
	"struct",
	"subprocess",
	"sys",
	"tempfile",
	"textwrap",
	"threading",
	"time",
	"timeit",
	"traceback",
	"types",
	"typing",
	"unicodedata",
	"unittest",
	"urllib",
	"uuid",
	"warnings",
	"weakref",
	"xml",
	"zipfile",
];

fn visibility_from_name(name: &[u8]) -> &'static [u8] {
	if name.len() >= 4 && name.starts_with(b"__") && name.ends_with(b"__") {
		return kinds::VIS_PUBLIC;
	}
	if name.starts_with(b"__") {
		return kinds::VIS_PRIVATE;
	}
	if name.starts_with(b"_") {
		return kinds::VIS_MODULE;
	}
	kinds::VIS_PUBLIC
}

fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::FUNCTION || last.kind == kinds::METHOD
}

fn is_class_scope(scope: &Moniker) -> bool {
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::CLASS
}

pub(super) fn first_docstring<'src>(body: Node<'src>) -> Option<Node<'src>> {
	let mut cursor = body.walk();
	let first = body.named_children(&mut cursor).next()?;
	if first.kind() != "expression_statement" {
		return None;
	}
	let mut inner = first.walk();
	let expr = first.named_children(&mut inner).next()?;
	if matches!(expr.kind(), "string" | "concatenated_string") {
		Some(expr)
	} else {
		None
	}
}

pub(super) fn emit_docstring_def(node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
	let m =
		crate::lang::callable::extend_segment_u32(parent, kinds::COMMENT, node.start_byte() as u32);
	let _ = graph.add_def(m, kinds::COMMENT, parent, Some(node_position(node)));
}
