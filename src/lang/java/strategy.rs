use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{
	CallableSlot, callable_segment_slots, extend_callable_slots, extend_segment,
	join_bytes_with_comma,
};
use crate::lang::strategy::{LangStrategy, NodeShape, RefSpec, Symbol};
use crate::lang::tree_util::{node_position, node_slice};

use super::kinds;

pub(super) struct Strategy<'src> {
	pub(super) module: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	#[allow(dead_code)]
	pub(super) presets: &'src super::Presets,
	pub(super) imports: RefCell<HashMap<Vec<u8>, &'static [u8]>>,
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
			"line_comment" | "block_comment" => NodeShape::Annotation {
				kind: kinds::COMMENT,
			},
			"package_declaration" => NodeShape::Skip,
			"import_declaration" => {
				self.handle_import(node, scope, graph);
				NodeShape::Skip
			}
			"class_declaration" => self.classify_type(node, scope, source, kinds::CLASS),
			"interface_declaration" => self.classify_type(node, scope, source, kinds::INTERFACE),
			"enum_declaration" => self.classify_enum(node, scope, source),
			"record_declaration" => self.classify_type(node, scope, source, kinds::RECORD),
			"annotation_type_declaration" => {
				self.classify_type(node, scope, source, kinds::ANNOTATION_TYPE)
			}
			"method_declaration" => self.classify_callable(node, scope, source, kinds::METHOD),
			"constructor_declaration" => {
				self.classify_callable(node, scope, source, kinds::CONSTRUCTOR)
			}
			"field_declaration" => {
				self.handle_field(node, scope, graph);
				NodeShape::Skip
			}
			"local_variable_declaration" => {
				self.handle_local_variable(node, scope, graph);
				NodeShape::Skip
			}
			"catch_formal_parameter" => {
				self.handle_catch_param(node, scope, graph);
				NodeShape::Skip
			}
			"enhanced_for_statement" => {
				self.handle_enhanced_for(node, scope, graph);
				NodeShape::Skip
			}
			"lambda_expression" => {
				self.handle_lambda(node, scope, graph);
				NodeShape::Skip
			}
			"method_invocation" => {
				self.handle_method_invocation(node, scope, graph);
				NodeShape::Skip
			}
			"object_creation_expression" => {
				self.handle_object_creation(node, scope, graph);
				NodeShape::Skip
			}
			"marker_annotation" | "annotation" => {
				self.handle_annotation(node, scope, graph);
				NodeShape::Skip
			}
			"identifier" => {
				self.handle_identifier(node, scope, graph);
				NodeShape::Skip
			}
			"type_identifier" | "scoped_type_identifier" | "generic_type" => {
				self.emit_uses_type(node, scope, graph);
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
		if kind == kinds::METHOD || kind == kinds::CONSTRUCTOR {
			if let Some(rt) = node.child_by_field_name("type") {
				self.emit_uses_type(rt, moniker, graph);
			}
			if let Some(params) = node.child_by_field_name("parameters") {
				self.emit_param_defs_and_types(params, moniker, graph);
			}
			return;
		}
		if kind == kinds::ENUM {
			self.emit_enum_constants(node, moniker, graph);
		}
	}

	fn after_body(&self, kind: &[u8], _moniker: &Moniker) {
		if kind == kinds::METHOD || kind == kinds::CONSTRUCTOR {
			self.pop_local_scope();
		}
	}
}

impl<'src_lang> Strategy<'src_lang> {
	fn classify_type<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		kind: &'static [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kind, name);

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"superclass" => {
					self.collect_heritage_refs(child, kinds::EXTENDS, &mut annotated_by)
				}
				"super_interfaces" | "extends_interfaces" => {
					self.collect_heritage_refs(child, kinds::IMPLEMENTS, &mut annotated_by)
				}
				_ => {}
			}
		}
		self.collect_annotations_from(node, &mut annotated_by);

		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility: modifier_visibility(node),
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

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() == "super_interfaces" {
				self.collect_heritage_refs(child, kinds::IMPLEMENTS, &mut annotated_by);
			}
		}
		self.collect_annotations_from(node, &mut annotated_by);

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::ENUM,
			visibility: modifier_visibility(node),
			signature: None,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_callable<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		kind: &'static [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let slots = formal_parameter_slots(node, source);
		let signature =
			join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
		let moniker = extend_callable_slots(scope, kind, name, &slots);

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		self.collect_annotations_from(node, &mut annotated_by);

		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			self.record_param_locals(params);
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility: modifier_visibility(node),
			signature: Some(signature),
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn emit_enum_constants(&self, enum_node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(body) = enum_node.child_by_field_name("body") else {
			return;
		};
		let mut cursor = body.walk();
		for child in body.children(&mut cursor) {
			if child.kind() != "enum_constant" {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			let m = extend_segment(parent, kinds::ENUM_CONSTANT, name);
			let _ = graph.add_def(m, kinds::ENUM_CONSTANT, parent, Some(node_position(child)));
		}
	}

	fn record_param_locals(&self, params: Node<'_>) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			if !matches!(child.kind(), "formal_parameter" | "spread_parameter") {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			self.record_local(name);
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
			if !matches!(child.kind(), "formal_parameter" | "spread_parameter") {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			if self.deep {
				let m = extend_segment(callable, kinds::PARAM, name);
				let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(child)));
			}
			if let Some(t) = child.child_by_field_name("type") {
				self.emit_uses_type(t, callable, graph);
			}
		}
	}

	fn handle_field(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let visibility = modifier_visibility(node);
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, parent, graph);
		}
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			let m = extend_segment(parent, kinds::FIELD, name);
			let attrs = DefAttrs {
				visibility,
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(
				m.clone(),
				kinds::FIELD,
				parent,
				Some(node_position(child)),
				&attrs,
			);
			let mut field_annotations: Vec<RefSpec> = Vec::new();
			self.collect_annotations_from(node, &mut field_annotations);
			for r in field_annotations {
				let attrs = RefAttrs {
					confidence: r.confidence,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(&m, r.target, r.kind, Some(r.position), &attrs);
			}
			if let Some(value) = child.child_by_field_name("value") {
				self.recurse_subtree(value, &m, graph);
			}
		}
	}

	fn handle_local_variable(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let inside_callable = is_callable_scope(scope, &self.module);
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			if inside_callable {
				self.record_local(name);
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, name);
					let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(child)));
				}
			}
			if let Some(value) = child.child_by_field_name("value") {
				self.recurse_subtree(value, scope, graph);
			}
		}
	}

	fn handle_catch_param(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let name = node_slice(name_node, self.source_bytes);
		if name.is_empty() {
			return;
		}
		if is_callable_scope(scope, &self.module) {
			self.record_local(name);
			if self.deep {
				let m = extend_segment(scope, kinds::PARAM, name);
				let _ = graph.add_def(m, kinds::PARAM, scope, Some(node_position(node)));
			}
		}
	}

	fn handle_enhanced_for(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		if let Some(name_node) = node.child_by_field_name("name") {
			let name = node_slice(name_node, self.source_bytes);
			if !name.is_empty() && is_callable_scope(scope, &self.module) {
				self.record_local(name);
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, name);
					let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(name_node)));
				}
			}
		}
		if let Some(value) = node.child_by_field_name("value") {
			self.recurse_subtree(value, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_children(body, scope, graph);
		}
	}

	fn handle_lambda(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			match params.kind() {
				"identifier" => {
					let name = node_slice(params, self.source_bytes);
					if !name.is_empty() {
						self.record_local(name);
						if self.deep {
							let m = extend_segment(scope, kinds::PARAM, name);
							let _ =
								graph.add_def(m, kinds::PARAM, scope, Some(node_position(params)));
						}
					}
				}
				"inferred_parameters" | "formal_parameters" => {
					let mut cursor = params.walk();
					for child in params.named_children(&mut cursor) {
						let name_node = match child.kind() {
							"identifier" => Some(child),
							"formal_parameter" | "spread_parameter" => {
								child.child_by_field_name("name")
							}
							_ => None,
						};
						let Some(nn) = name_node else { continue };
						let name = node_slice(nn, self.source_bytes);
						if name.is_empty() {
							continue;
						}
						self.record_local(name);
						if self.deep {
							let m = extend_segment(scope, kinds::PARAM, name);
							let _ = graph.add_def(m, kinds::PARAM, scope, Some(node_position(nn)));
						}
					}
				}
				_ => {}
			}
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_children(body, scope, graph);
		}
		self.pop_local_scope();
	}

	fn handle_import(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let mut wildcard = false;
		let mut path_node: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"asterisk" | "*" => wildcard = true,
				"scoped_identifier" | "identifier" => path_node = Some(c),
				_ => {}
			}
		}
		let Some(path_node) = path_node else { return };
		let dotted_bytes = node_slice(path_node, self.source_bytes);
		let dotted = std::str::from_utf8(dotted_bytes).unwrap_or("");
		if dotted.is_empty() {
			return;
		}

		let pieces: Vec<&str> = dotted.split('.').collect();
		let confidence = external_or_imported(&pieces);

		if wildcard {
			let target = wildcard_target(self.module.as_view().project(), &pieces, confidence);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
			return;
		}

		if let Some(last) = pieces.last().copied() {
			self.imports
				.borrow_mut()
				.insert(last.as_bytes().to_vec(), confidence);
		}
		let target = symbol_target(self.module.as_view().project(), &pieces, confidence);
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_SYMBOL, Some(pos), &attrs);
	}

	fn handle_method_invocation(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);

		let object = node.child_by_field_name("object");
		let Some(name_node) = node.child_by_field_name("name") else {
			self.walk_children(node, scope, graph);
			return;
		};
		let name = node_slice(name_node, self.source_bytes);
		if name.is_empty() {
			self.walk_children(node, scope, graph);
			return;
		}

		if let Some(obj) = object {
			let target = self.resolve_callable_target(scope, &obj, name, kinds::METHOD);
			let confidence = if obj.kind() == "identifier" {
				let obj_name = node_slice(obj, self.source_bytes);
				self.import_confidence_for(obj_name)
					.unwrap_or(kinds::CONF_NAME_MATCH)
			} else {
				kinds::CONF_NAME_MATCH
			};
			let attrs = RefAttrs {
				receiver_hint: receiver_hint(obj, self.source_bytes),
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::METHOD_CALL, Some(pos), &attrs);
			self.recurse_subtree(obj, scope, graph);
		} else {
			let confidence = match self.import_confidence_for(name) {
				Some(c) => Some(c),
				None => self.name_confidence(name),
			};
			if let Some(confidence) = confidence {
				let target = if confidence == kinds::CONF_LOCAL {
					extend_segment(scope, kinds::LOCAL, name)
				} else {
					self.lookup_callable_in_class(scope, name, kinds::METHOD)
						.unwrap_or_else(|| extend_segment(&self.module, kinds::METHOD, name))
				};
				let attrs = RefAttrs {
					confidence,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
			}
		}

		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk_children(args, scope, graph);
		}
	}

	fn lookup_callable_in_class(
		&self,
		scope: &Moniker,
		name: &[u8],
		kind: &[u8],
	) -> Option<Moniker> {
		let cls = enclosing_class(scope)?;
		let seg = self.callable_table.get(&(cls.clone(), name.to_vec()))?;
		Some(extend_segment(&cls, kind, seg))
	}

	fn resolve_callable_target(
		&self,
		scope: &Moniker,
		receiver: &Node<'_>,
		name: &[u8],
		kind: &[u8],
	) -> Moniker {
		match receiver.kind() {
			"this" | "super" => self
				.lookup_callable_in_class(scope, name, kind)
				.unwrap_or_else(|| extend_segment(&self.module, kind, name)),
			_ => extend_segment(&self.module, kind, name),
		}
	}

	fn handle_object_creation(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		if let Some(t) = node.child_by_field_name("type") {
			let name_str = match t.kind() {
				"type_identifier" => std::str::from_utf8(node_slice(t, self.source_bytes))
					.unwrap_or("")
					.to_string(),
				"scoped_type_identifier" => last_identifier(t, self.source_bytes).to_string(),
				"generic_type" => generic_type_short(t, self.source_bytes).to_string(),
				_ => String::new(),
			};
			if !name_str.is_empty() {
				let (target, confidence) =
					self.resolve_type_target(name_str.as_bytes(), kinds::CLASS);
				let attrs = RefAttrs {
					confidence,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::INSTANTIATES, Some(pos), &attrs);
			}
		}
		self.walk_children(node, scope, graph);
	}

	fn handle_annotation(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let name = annotation_name(name_node, self.source_bytes);
		if name.is_empty() {
			return;
		}
		let (target, confidence) =
			self.resolve_type_target(name.as_bytes(), kinds::ANNOTATION_TYPE);
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::ANNOTATES, Some(pos), &attrs);
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk_children(args, scope, graph);
		}
	}

	fn collect_annotations_from(&self, node: Node<'_>, out: &mut Vec<RefSpec>) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "modifiers" {
				continue;
			}
			let mut mc = child.walk();
			for m in child.children(&mut mc) {
				if !matches!(m.kind(), "marker_annotation" | "annotation") {
					continue;
				}
				let Some(name_node) = m.child_by_field_name("name") else {
					continue;
				};
				let name = annotation_name(name_node, self.source_bytes);
				if name.is_empty() {
					continue;
				}
				let (target, confidence) =
					self.resolve_type_target(name.as_bytes(), kinds::ANNOTATION_TYPE);
				out.push(RefSpec {
					kind: kinds::ANNOTATES,
					target,
					confidence,
					position: node_position(m),
					receiver_hint: b"",
					alias: b"",
				});
			}
		}
	}

	fn collect_heritage_refs(&self, clause: Node<'_>, edge: &'static [u8], out: &mut Vec<RefSpec>) {
		let mut cursor = clause.walk();
		for child in clause.named_children(&mut cursor) {
			let name = match child.kind() {
				"type_identifier" => {
					std::str::from_utf8(node_slice(child, self.source_bytes)).unwrap_or("")
				}
				"scoped_type_identifier" => last_identifier(child, self.source_bytes),
				"generic_type" => generic_type_short(child, self.source_bytes),
				"type_list" => {
					self.collect_heritage_refs(child, edge, out);
					continue;
				}
				_ => continue,
			};
			if name.is_empty() {
				continue;
			}
			let target_kind = if edge == kinds::IMPLEMENTS {
				kinds::INTERFACE
			} else {
				kinds::CLASS
			};
			let (target, confidence) = self.resolve_type_target(name.as_bytes(), target_kind);
			out.push(RefSpec {
				kind: edge,
				target,
				confidence,
				position: node_position(child),
				receiver_hint: b"",
				alias: b"",
			});
		}
	}

	fn emit_uses_type(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let name = match node.kind() {
			"type_identifier" => std::str::from_utf8(node_slice(node, self.source_bytes))
				.unwrap_or("")
				.to_string(),
			"scoped_type_identifier" => last_identifier(node, self.source_bytes).to_string(),
			"generic_type" => {
				let head = generic_type_short(node, self.source_bytes).to_string();
				if let Some(args) = node.child_by_field_name("type_arguments") {
					self.walk_children(args, scope, graph);
				}
				head
			}
			"array_type" => {
				if let Some(elt) = node.child_by_field_name("element") {
					self.emit_uses_type(elt, scope, graph);
				}
				return;
			}
			_ => {
				self.walk_children(node, scope, graph);
				return;
			}
		};
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
			extend_segment(&self.module, kinds::FIELD, name)
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

	fn import_confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.imports.borrow().get(name).copied()
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
}

pub(super) fn collect_type_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	out: &mut HashMap<&'src [u8], Moniker>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		let kind: Option<&[u8]> = match child.kind() {
			"class_declaration" => Some(kinds::CLASS),
			"interface_declaration" => Some(kinds::INTERFACE),
			"enum_declaration" => Some(kinds::ENUM),
			"record_declaration" => Some(kinds::RECORD),
			"annotation_type_declaration" => Some(kinds::ANNOTATION_TYPE),
			_ => None,
		};
		let Some(kind) = kind else {
			collect_type_table(child, source, parent, out);
			continue;
		};
		let Some(name_node) = child.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, source);
		let m = extend_segment(parent, kind, name);
		out.entry(name).or_insert_with(|| m.clone());
		if let Some(body) = child.child_by_field_name("body") {
			collect_type_table(body, source, &m, out);
		}
	}
}

fn formal_parameter_slots(callable: Node<'_>, source: &[u8]) -> Vec<CallableSlot> {
	let Some(params) = callable.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let mut cursor = params.walk();
	for c in params.named_children(&mut cursor) {
		if !matches!(c.kind(), "formal_parameter" | "spread_parameter") {
			continue;
		}
		let r#type = c
			.child_by_field_name("type")
			.and_then(|t| t.utf8_text(source).ok())
			.map(crate::lang::callable::normalize_type_text)
			.unwrap_or_default();
		let name = c
			.child_by_field_name("name")
			.map(|n| node_slice(n, source).to_vec())
			.unwrap_or_default();
		out.push(CallableSlot { name, r#type });
	}
	out
}

fn slot_signature_bytes(slot: &CallableSlot) -> Vec<u8> {
	match (slot.name.as_slice(), slot.r#type.as_slice()) {
		(b"", b"") => b"_".to_vec(),
		(name, b"") => name.to_vec(),
		(b"", ty) => ty.to_vec(),
		(name, ty) => {
			let mut out = Vec::with_capacity(name.len() + 1 + ty.len());
			out.extend_from_slice(name);
			out.push(b':');
			out.extend_from_slice(ty);
			out
		}
	}
}

fn enclosing_class(scope: &Moniker) -> Option<Moniker> {
	let view = scope.as_view();
	let segs: Vec<_> = view.segments().collect();
	let last_class_idx = segs.iter().rposition(|s| {
		matches!(
			s.kind,
			b"class" | b"interface" | b"enum" | b"record" | b"annotation_type"
		)
	})?;
	let mut b = crate::core::moniker::MonikerBuilder::new();
	b.project(view.project());
	for s in &segs[..=last_class_idx] {
		b.segment(s.kind, s.name);
	}
	Some(b.build())
}

pub(super) fn collect_callable_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	out: &mut std::collections::HashMap<(Moniker, Vec<u8>), Vec<u8>>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		match child.kind() {
			"class_declaration"
			| "interface_declaration"
			| "enum_declaration"
			| "record_declaration"
			| "annotation_type_declaration" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let kind: &[u8] = match child.kind() {
					"class_declaration" => kinds::CLASS,
					"interface_declaration" => kinds::INTERFACE,
					"enum_declaration" => kinds::ENUM,
					"record_declaration" => kinds::RECORD,
					"annotation_type_declaration" => kinds::ANNOTATION_TYPE,
					_ => unreachable!(),
				};
				let name = node_slice(name_node, source);
				let scope = extend_segment(parent, kind, name);
				if let Some(body) = child.child_by_field_name("body") {
					collect_callable_table(body, source, &scope, out);
				}
			}
			"method_declaration" | "constructor_declaration" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let name = node_slice(name_node, source);
				let slots = formal_parameter_slots(child, source);
				let seg = callable_segment_slots(name, &slots);
				out.insert((parent.clone(), name.to_vec()), seg);
			}
			_ => {
				collect_callable_table(child, source, parent, out);
			}
		}
	}
}

fn modifier_visibility(node: Node<'_>) -> &'static [u8] {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() != "modifiers" {
			continue;
		}
		let mut mc = child.walk();
		for m in child.children(&mut mc) {
			match m.kind() {
				"public" => return kinds::VIS_PUBLIC,
				"protected" => return kinds::VIS_PROTECTED,
				"private" => return kinds::VIS_PRIVATE,
				_ => {}
			}
		}
	}
	kinds::VIS_PACKAGE
}

fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::METHOD || last.kind == kinds::CONSTRUCTOR
}

fn receiver_hint<'a>(obj: Node<'a>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SUPER, HINT_THIS};
	match obj.kind() {
		"this" => HINT_THIS,
		"super" => HINT_SUPER,
		"identifier" => node_slice(obj, source),
		"method_invocation" => HINT_CALL,
		"field_access" => HINT_MEMBER,
		"scoped_identifier" => HINT_MEMBER,
		_ => b"",
	}
}

fn annotation_name<'a>(name_node: Node<'a>, source: &'a [u8]) -> &'a str {
	match name_node.kind() {
		"identifier" => std::str::from_utf8(node_slice(name_node, source)).unwrap_or(""),
		"scoped_identifier" => last_identifier(name_node, source),
		_ => "",
	}
}

fn last_identifier<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
	if let Some(name) = node.child_by_field_name("name") {
		return name.utf8_text(source).unwrap_or("");
	}
	let mut cursor = node.walk();
	let mut last = "";
	for c in node.named_children(&mut cursor) {
		if matches!(c.kind(), "type_identifier" | "identifier") {
			last = c.utf8_text(source).unwrap_or(last);
		}
	}
	last
}

fn generic_type_short<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
	let mut cursor = node.walk();
	for c in node.named_children(&mut cursor) {
		match c.kind() {
			"type_identifier" => return c.utf8_text(source).unwrap_or(""),
			"scoped_type_identifier" => return last_identifier(c, source),
			_ => {}
		}
	}
	""
}

fn wildcard_target(project: &[u8], pieces: &[&str], confidence: &[u8]) -> Moniker {
	if confidence == kinds::CONF_IMPORTED && !pieces.is_empty() {
		let mut b = MonikerBuilder::new();
		b.project(project);
		b.segment(crate::lang::kinds::LANG, b"java");
		for piece in pieces {
			b.segment(kinds::PACKAGE, piece.as_bytes());
		}
		return b.build();
	}
	external_package_target(project, pieces)
}

fn symbol_target(project: &[u8], pieces: &[&str], confidence: &[u8]) -> Moniker {
	if confidence == kinds::CONF_IMPORTED && !pieces.is_empty() {
		let mut b = MonikerBuilder::new();
		b.project(project);
		b.segment(crate::lang::kinds::LANG, b"java");
		let last = pieces.len() - 1;
		for (i, piece) in pieces.iter().enumerate() {
			let kind = if i == last {
				kinds::MODULE
			} else {
				kinds::PACKAGE
			};
			b.segment(kind, piece.as_bytes());
		}
		b.segment(kinds::PATH, pieces[last].as_bytes());
		return b.build();
	}
	external_package_target(project, pieces)
}

fn external_package_target(project: &[u8], pieces: &[&str]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(project);
	if pieces.is_empty() {
		return b.build();
	}
	b.segment(kinds::EXTERNAL_PKG, pieces[0].as_bytes());
	for piece in &pieces[1..] {
		b.segment(kinds::PATH, piece.as_bytes());
	}
	b.build()
}

fn external_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	match pieces[0] {
		"java" | "javax" | "kotlin" | "sun" => kinds::CONF_EXTERNAL,
		"com" if pieces.get(1).copied() == Some("sun") => kinds::CONF_EXTERNAL,
		_ => kinds::CONF_IMPORTED,
	}
}
