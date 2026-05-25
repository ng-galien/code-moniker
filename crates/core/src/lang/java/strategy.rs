// code-moniker: ignore-file[smell-feature-envy-local, smell-long-parameter-list, smell-data-clumps-param-names, smell-god-type-local-metrics, smell-large-type]
// TODO(smell): split Java Strategy into classification, graph emission, type resolution, and table-building phases before enabling these guardrails here.

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

use super::{builtins, kinds};

pub(super) type CallableTable = HashMap<(Moniker, Vec<u8>, usize), Vec<u8>>;
pub(super) type ReturnTypeTable = HashMap<(Moniker, Vec<u8>, usize), Moniker>;
pub(super) type ValueTypeTable = HashMap<(Moniker, Vec<u8>), Moniker>;
pub(super) type ImportConfidenceTable = HashMap<Vec<u8>, &'static [u8]>;
pub(super) type ImportTargetTable = HashMap<Vec<u8>, Moniker>;

pub(super) struct Strategy<'src> {
	pub(super) module: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	#[allow(dead_code)]
	pub(super) presets: &'src super::Presets,
	pub(super) imports: RefCell<HashMap<Vec<u8>, &'static [u8]>>,
	pub(super) import_targets: RefCell<HashMap<Vec<u8>, Moniker>>,
	pub(super) local_scope: RefCell<Vec<HashSet<Vec<u8>>>>,
	pub(super) local_types: RefCell<Vec<HashMap<Vec<u8>, Option<Moniker>>>>,
	pub(super) type_table: HashMap<&'src [u8], Moniker>,
	pub(super) callable_table: CallableTable,
	pub(super) return_type_table: ReturnTypeTable,
	pub(super) field_types: ValueTypeTable,
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
			"for_statement" => {
				self.handle_for_statement(node, scope, graph);
				NodeShape::Skip
			}
			"block" => {
				self.handle_block(node, scope, graph);
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
			"method_reference" => {
				self.handle_method_reference(node, scope, graph);
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
		if kind == kinds::RECORD {
			self.emit_record_components(node, moniker, graph);
		}
	}

	fn after_body(&self, kind: &[u8], _moniker: &Moniker) {
		if kind == kinds::METHOD || kind == kinds::CONSTRUCTOR {
			self.pop_local_scope();
		}
	}

	fn on_symbol_emitted(
		&self,
		node: Node<'_>,
		sym_kind: &[u8],
		sym_moniker: &Moniker,
		source: &[u8],
		graph: &mut CodeGraph,
	) {
		if !matches!(sym_kind, kinds::METHOD | kinds::CONSTRUCTOR) {
			return;
		}
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let name = node_slice(name_node, source);
		let arity = formal_parameter_slots(node, source).len();
		graph.set_def_call_metadata(sym_moniker, name, arity);
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

	fn emit_record_components(
		&self,
		record_node: Node<'_>,
		parent: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(params) = record_node.child_by_field_name("parameters") else {
			return;
		};
		let explicit_accessors = explicit_no_arg_methods(record_node, self.source_bytes);
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			if !matches!(child.kind(), "formal_parameter" | "spread_parameter") {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			if name.is_empty() {
				continue;
			}
			if let Some(t) = child.child_by_field_name("type") {
				self.emit_uses_type(t, parent, graph);
			}

			let field = extend_segment(parent, kinds::FIELD, name);
			let field_attrs = DefAttrs {
				visibility: kinds::VIS_PRIVATE,
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(
				field.clone(),
				kinds::FIELD,
				parent,
				Some(node_position(child)),
				&field_attrs,
			);
			self.emit_component_annotations(child, &field, graph);

			if explicit_accessors.contains(name) {
				continue;
			}
			let accessor = extend_callable_slots(parent, kinds::METHOD, name, &[]);
			let accessor_attrs = DefAttrs {
				visibility: kinds::VIS_PUBLIC,
				call_name: name,
				call_arity: Some(0),
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(
				accessor.clone(),
				kinds::METHOD,
				parent,
				Some(node_position(child)),
				&accessor_attrs,
			);
			if let Some(t) = child.child_by_field_name("type") {
				self.emit_uses_type(t, &accessor, graph);
			}
			self.emit_component_annotations(child, &accessor, graph);
		}
	}

	fn emit_component_annotations(
		&self,
		component: Node<'_>,
		source: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut annotations: Vec<RefSpec> = Vec::new();
		self.collect_annotations_from(component, &mut annotations);
		for r in annotations {
			let attrs = RefAttrs {
				confidence: r.confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(source, r.target, r.kind, Some(r.position), &attrs);
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
			let declared_type = child
				.child_by_field_name("type")
				.and_then(|t| self.type_target_for_node(t));
			self.record_local_type(name, declared_type);
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
		let declared_type = if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
			self.type_target_for_node(t)
		} else {
			None
		};
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
				self.record_local_type(name, declared_type.clone());
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
		let declared_type = if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
			self.type_target_for_node(t)
		} else {
			None
		};
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let name = node_slice(name_node, self.source_bytes);
		if name.is_empty() {
			return;
		}
		if is_callable_scope(scope, &self.module) {
			self.record_local_type(name, declared_type);
			if self.deep {
				let m = extend_segment(scope, kinds::PARAM, name);
				let _ = graph.add_def(m, kinds::PARAM, scope, Some(node_position(node)));
			}
		}
	}

	fn handle_enhanced_for(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let declared_type = if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
			self.type_target_for_node(t)
		} else {
			None
		};
		if let Some(value) = node.child_by_field_name("value") {
			self.recurse_subtree(value, scope, graph);
		}
		self.push_local_scope();
		if let Some(name_node) = node.child_by_field_name("name") {
			let name = node_slice(name_node, self.source_bytes);
			if !name.is_empty() && is_callable_scope(scope, &self.module) {
				self.record_local_type(name, declared_type);
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, name);
					let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(name_node)));
				}
			}
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_children(body, scope, graph);
		}
		self.pop_local_scope();
	}

	fn handle_for_statement(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.push_local_scope();
		self.walk_children(node, scope, graph);
		self.pop_local_scope();
	}

	fn handle_block(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.push_local_scope();
		self.walk_children(node, scope, graph);
		self.pop_local_scope();
	}

	fn handle_lambda(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			match params.kind() {
				"identifier" => {
					let name = node_slice(params, self.source_bytes);
					if !name.is_empty() {
						self.record_local_type(name, None);
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
						let declared_type = child
							.child_by_field_name("type")
							.and_then(|t| self.type_target_for_node(t));
						self.record_local_type(name, declared_type);
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
			if body.kind() == "block" {
				self.walk_children(body, scope, graph);
			} else {
				self.recurse_subtree(body, scope, graph);
			}
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
			let target = wildcard_target(&self.module, &pieces, confidence);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
			return;
		}

		let target = symbol_target(&self.module, &pieces, confidence);
		if let Some(last) = pieces.last().copied() {
			let last_bytes = last.as_bytes();
			self.imports
				.borrow_mut()
				.insert(last_bytes.to_vec(), confidence);
			self.import_targets
				.borrow_mut()
				.insert(last_bytes.to_vec(), target.clone());
		}
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
			let arguments = node.child_by_field_name("arguments");
			let arity = arguments.map(argument_count).unwrap_or(0);
			let (target, confidence) =
				self.resolve_callable_target(scope, &obj, name, kinds::METHOD, arity);
			let attrs = RefAttrs {
				receiver_hint: receiver_hint(obj, self.source_bytes),
				confidence,
				call_name: name,
				call_arity: Some(arity),
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::METHOD_CALL, Some(pos), &attrs);
			self.recurse_subtree(obj, scope, graph);
		} else {
			let arguments = node.child_by_field_name("arguments");
			let arity = arguments.map(argument_count).unwrap_or(0);
			let confidence = match self.import_confidence_for(name) {
				Some(c) => Some(c),
				None => self.name_confidence(name),
			};
			if let Some(confidence) = confidence {
				let target = if confidence == kinds::CONF_LOCAL {
					extend_segment(scope, kinds::LOCAL, name)
				} else {
					self.lookup_callable_in_class(scope, name, kinds::METHOD, arity)
						.unwrap_or_else(|| extend_segment(&self.module, kinds::METHOD, name))
				};
				let attrs = RefAttrs {
					confidence,
					call_name: name,
					call_arity: Some(arity),
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
			}
		}

		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk_children(args, scope, graph);
		}
	}

	fn handle_method_reference(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let mut cursor = node.walk();
		let named: Vec<_> = node
			.named_children(&mut cursor)
			.filter(|child| child.kind() != "type_arguments")
			.collect();
		let Some(method_idx) = named.iter().rposition(|child| child.kind() == "identifier") else {
			self.walk_children(node, scope, graph);
			return;
		};
		let Some(receiver) = named.iter().enumerate().find_map(|(idx, child)| {
			if idx == method_idx {
				None
			} else {
				Some(*child)
			}
		}) else {
			self.walk_children(node, scope, graph);
			return;
		};
		let method_name = node_slice(named[method_idx], self.source_bytes);
		if method_name.is_empty() {
			self.walk_children(node, scope, graph);
			return;
		}

		if matches!(
			receiver.kind(),
			"type_identifier" | "scoped_type_identifier" | "generic_type" | "array_type"
		) {
			self.emit_uses_type(receiver, scope, graph);
		}
		let Some((owner, confidence)) = self.method_reference_receiver_target(scope, receiver)
		else {
			let attrs = RefAttrs {
				receiver_hint: receiver_hint(receiver, self.source_bytes),
				confidence: kinds::CONF_NAME_MATCH,
				call_name: method_name,
				..RefAttrs::default()
			};
			let target = extend_segment(&self.module, kinds::METHOD, method_name);
			let _ = graph.add_ref_attrs(scope, target, kinds::METHOD_CALL, Some(pos), &attrs);
			return;
		};
		let target = self
			.lookup_unique_callable_in_type(&owner, method_name, kinds::METHOD)
			.unwrap_or_else(|| extend_segment(&owner, kinds::METHOD, method_name));
		let attrs = RefAttrs {
			receiver_hint: receiver_hint(receiver, self.source_bytes),
			confidence,
			call_name: method_name,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::METHOD_CALL, Some(pos), &attrs);
	}

	fn lookup_callable_in_class(
		&self,
		scope: &Moniker,
		name: &[u8],
		kind: &[u8],
		arity: usize,
	) -> Option<Moniker> {
		let cls = enclosing_class(scope)?;
		let key = (cls.clone(), name.to_vec(), arity);
		let seg = self.callable_table.get(&key)?;
		Some(extend_segment(&cls, kind, seg))
	}

	fn lookup_callable_in_type(
		&self,
		owner: &Moniker,
		name: &[u8],
		kind: &[u8],
		arity: usize,
	) -> Option<Moniker> {
		let key = (owner.clone(), name.to_vec(), arity);
		let seg = self.callable_table.get(&key)?;
		Some(extend_segment(owner, kind, seg))
	}

	fn lookup_unique_callable_in_type(
		&self,
		owner: &Moniker,
		name: &[u8],
		kind: &[u8],
	) -> Option<Moniker> {
		let mut found: Option<&Vec<u8>> = None;
		for ((candidate_owner, candidate_name, _arity), seg) in &self.callable_table {
			if candidate_owner != owner || candidate_name.as_slice() != name {
				continue;
			}
			if found.is_some() {
				return None;
			}
			found = Some(seg);
		}
		found.map(|seg| extend_segment(owner, kind, seg))
	}

	fn resolve_callable_target(
		&self,
		scope: &Moniker,
		receiver: &Node<'_>,
		name: &[u8],
		kind: &[u8],
		arity: usize,
	) -> (Moniker, &'static [u8]) {
		match receiver.kind() {
			"this" | "super" => self
				.lookup_callable_in_class(scope, name, kind, arity)
				.map(|target| (target, kinds::CONF_RESOLVED))
				.unwrap_or_else(|| {
					(
						extend_segment(&self.module, kind, name),
						kinds::CONF_NAME_MATCH,
					)
				}),
			"identifier" => {
				let receiver_name = node_slice(*receiver, self.source_bytes);
				if let Some(owner) = self.lookup_value_type(scope, receiver_name) {
					if let Some(target) = self.lookup_callable_in_type(&owner, name, kind, arity) {
						return (target, kinds::CONF_RESOLVED);
					}
					let confidence = self.type_owner_confidence(&owner);
					return (extend_arity_call(&owner, kind, name, arity), confidence);
				}
				if let Some((owner, confidence)) = self.lookup_known_type_name(receiver_name) {
					return (extend_arity_call(&owner, kind, name, arity), confidence);
				}
				let confidence = self
					.import_confidence_for(receiver_name)
					.unwrap_or(kinds::CONF_NAME_MATCH);
				(extend_segment(&self.module, kind, name), confidence)
			}
			"object_creation_expression" => {
				if let Some(owner) = self.type_target_for_object_creation(*receiver) {
					if let Some(target) = self.lookup_callable_in_type(&owner, name, kind, arity) {
						return (target, kinds::CONF_RESOLVED);
					}
					let confidence = self.type_owner_confidence(&owner);
					return (extend_arity_call(&owner, kind, name, arity), confidence);
				}
				(
					extend_segment(&self.module, kind, name),
					kinds::CONF_NAME_MATCH,
				)
			}
			"method_invocation" => {
				if let Some(owner) = self.expression_type_target(scope, *receiver) {
					if let Some(target) = self.lookup_callable_in_type(&owner, name, kind, arity) {
						return (target, kinds::CONF_RESOLVED);
					}
					let confidence = self.type_owner_confidence(&owner);
					return (extend_arity_call(&owner, kind, name, arity), confidence);
				}
				if let Some(owner) = self.expression_external_owner(scope, *receiver) {
					return (
						extend_arity_call(&owner, kind, name, arity),
						kinds::CONF_EXTERNAL,
					);
				}
				(
					extend_arity_call(&self.module, kind, name, arity),
					kinds::CONF_EXTERNAL,
				)
			}
			"field_access" | "scoped_identifier" => {
				if let Some(owner) = self.expression_external_owner(scope, *receiver) {
					return (
						extend_arity_call(&owner, kind, name, arity),
						kinds::CONF_EXTERNAL,
					);
				}
				(
					extend_segment(&self.module, kind, name),
					kinds::CONF_NAME_MATCH,
				)
			}
			"string_literal" => (
				extend_arity_call(&self.java_lang_type_target(b"String"), kind, name, arity),
				kinds::CONF_EXTERNAL,
			),
			_ => (
				extend_segment(&self.module, kind, name),
				kinds::CONF_NAME_MATCH,
			),
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
		if name.is_empty() || builtins::is_primitive_type(name.as_bytes()) {
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
		if let Some(target) = self.external_field_access_target(scope, node) {
			let attrs = RefAttrs {
				confidence: kinds::CONF_EXTERNAL,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				scope,
				target,
				kinds::READS,
				Some(node_position(node)),
				&attrs,
			);
			return;
		}
		if let Some((target, confidence)) = self.lookup_known_type_name(name)
			&& confidence == kinds::CONF_EXTERNAL
		{
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
			return;
		}
		if let Some(target) = self.lookup_field(scope, name) {
			let attrs = RefAttrs {
				confidence: kinds::CONF_RESOLVED,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				scope,
				target,
				kinds::READS,
				Some(node_position(node)),
				&attrs,
			);
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
		self.local_types.borrow_mut().push(HashMap::new());
	}

	fn pop_local_scope(&self) {
		self.local_scope.borrow_mut().pop();
		self.local_types.borrow_mut().pop();
	}

	fn record_local(&self, name: &[u8]) {
		if let Some(top) = self.local_scope.borrow_mut().last_mut() {
			top.insert(name.to_vec());
		}
	}

	fn record_local_type(&self, name: &[u8], type_target: Option<Moniker>) {
		self.record_local(name);
		if let Some(top) = self.local_types.borrow_mut().last_mut() {
			top.insert(name.to_vec(), type_target);
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

	fn type_owner_confidence(&self, owner: &Moniker) -> &'static [u8] {
		if java_external_target_shape(owner) {
			kinds::CONF_EXTERNAL
		} else if self.type_table.values().any(|m| m == owner) {
			kinds::CONF_RESOLVED
		} else if same_package_target_shape(&self.module, owner) {
			kinds::CONF_NAME_MATCH
		} else {
			kinds::CONF_IMPORTED
		}
	}

	fn lookup_value_type(&self, scope: &Moniker, name: &[u8]) -> Option<Moniker> {
		for frame in self.local_types.borrow().iter().rev() {
			if let Some(value) = frame.get(name) {
				return value.clone();
			}
		}
		let cls = enclosing_class(scope)?;
		self.field_types.get(&(cls, name.to_vec())).cloned()
	}

	fn lookup_field(&self, scope: &Moniker, name: &[u8]) -> Option<Moniker> {
		let cls = enclosing_class(scope)?;
		self.field_types
			.contains_key(&(cls.clone(), name.to_vec()))
			.then(|| extend_segment(&cls, kinds::FIELD, name))
	}

	fn method_reference_receiver_target(
		&self,
		scope: &Moniker,
		receiver: Node<'_>,
	) -> Option<(Moniker, &'static [u8])> {
		match receiver.kind() {
			"this" | "super" => Some((enclosing_class(scope)?, kinds::CONF_RESOLVED)),
			"identifier" => {
				let name = node_slice(receiver, self.source_bytes);
				if let Some(target) = self.lookup_value_type(scope, name) {
					return Some((target, kinds::CONF_RESOLVED));
				}
				self.lookup_known_type_name(name)
			}
			"type_identifier" | "scoped_type_identifier" | "generic_type" | "array_type" => {
				let name = type_name_bytes(receiver, self.source_bytes)?;
				if name.is_empty() || builtins::is_primitive_type(&name) {
					return None;
				}
				Some(self.resolve_type_target(&name, kinds::CLASS))
			}
			_ => None,
		}
	}

	fn lookup_known_type_name(&self, name: &[u8]) -> Option<(Moniker, &'static [u8])> {
		if let Some(target) = self.type_table.get(name) {
			return Some((target.clone(), kinds::CONF_RESOLVED));
		}
		if let Some(target) = self.lookup_import_target(name) {
			let confidence = self
				.import_confidence_for(name)
				.unwrap_or(kinds::CONF_NAME_MATCH);
			return Some((target, confidence));
		}
		if builtins::is_java_lang_type(name) {
			let target = symbol_target(
				&self.module,
				&["java", "lang", std::str::from_utf8(name).unwrap_or("")],
				kinds::CONF_EXTERNAL,
			);
			return Some((target, kinds::CONF_EXTERNAL));
		}
		None
	}

	fn lookup_import_target(&self, name: &[u8]) -> Option<Moniker> {
		self.import_targets.borrow().get(name).cloned()
	}

	fn type_target_for_node(&self, node: Node<'_>) -> Option<Moniker> {
		let name = type_name_bytes(node, self.source_bytes)?;
		if name.is_empty() || builtins::is_primitive_type(&name) {
			return None;
		}
		Some(self.resolve_type_target(&name, kinds::CLASS).0)
	}

	fn type_target_for_object_creation(&self, node: Node<'_>) -> Option<Moniker> {
		node.child_by_field_name("type")
			.and_then(|t| self.type_target_for_node(t))
	}

	fn expression_type_target(&self, scope: &Moniker, node: Node<'_>) -> Option<Moniker> {
		match node.kind() {
			"identifier" => self.lookup_value_type(scope, node_slice(node, self.source_bytes)),
			"this" | "super" => enclosing_class(scope),
			"object_creation_expression" => self.type_target_for_object_creation(node),
			"method_invocation" => self.method_invocation_return_type(scope, node),
			_ => None,
		}
	}

	fn expression_external_owner(&self, scope: &Moniker, node: Node<'_>) -> Option<Moniker> {
		match node.kind() {
			"identifier" => {
				let name = node_slice(node, self.source_bytes);
				if let Some(target) = self.lookup_value_type(scope, name)
					&& java_external_target_shape(&target)
				{
					return Some(target);
				}
				match self.lookup_known_type_name(name) {
					Some((target, kinds::CONF_EXTERNAL)) => Some(target),
					_ => None,
				}
			}
			"string_literal" => Some(self.java_lang_type_target(b"String")),
			"type_identifier" | "scoped_type_identifier" | "generic_type" | "array_type" => {
				let name = type_name_bytes(node, self.source_bytes)?;
				let (target, confidence) = self.resolve_type_target(&name, kinds::CLASS);
				(confidence == kinds::CONF_EXTERNAL).then_some(target)
			}
			"field_access" | "scoped_identifier" => node
				.child_by_field_name("object")
				.and_then(|object| self.expression_external_owner(scope, object)),
			"method_invocation" => self
				.expression_type_target(scope, node)
				.filter(java_external_target_shape)
				.or_else(|| {
					node.child_by_field_name("object")
						.and_then(|object| self.expression_external_owner(scope, object))
				}),
			_ => None,
		}
	}

	fn external_field_access_target(&self, scope: &Moniker, node: Node<'_>) -> Option<Moniker> {
		let parent = node.parent()?;
		if parent.kind() != "field_access" && parent.kind() != "scoped_identifier" {
			return None;
		}
		let field = parent.child_by_field_name("field")?;
		if !same_node(field, node) {
			return None;
		}
		let object = parent.child_by_field_name("object")?;
		let owner = self.expression_external_owner(scope, object)?;
		Some(extend_segment(
			&owner,
			kinds::FIELD,
			node_slice(node, self.source_bytes),
		))
	}

	fn java_lang_type_target(&self, name: &[u8]) -> Moniker {
		symbol_target(
			&self.module,
			&["java", "lang", std::str::from_utf8(name).unwrap_or("")],
			kinds::CONF_EXTERNAL,
		)
	}

	fn method_invocation_return_type(&self, scope: &Moniker, node: Node<'_>) -> Option<Moniker> {
		let name_node = node.child_by_field_name("name")?;
		let name = node_slice(name_node, self.source_bytes);
		if name.is_empty() {
			return None;
		}
		let arity = node
			.child_by_field_name("arguments")
			.map(argument_count)
			.unwrap_or(0);
		let owner = match node.child_by_field_name("object") {
			Some(obj) => self.expression_type_target(scope, obj)?,
			None => enclosing_class(scope)?,
		};
		self.return_type_table
			.get(&(owner, name.to_vec(), arity))
			.cloned()
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
		if builtins::is_java_lang_type(name) {
			let target = symbol_target(
				&self.module,
				&["java", "lang", std::str::from_utf8(name).unwrap_or("")],
				kinds::CONF_EXTERNAL,
			);
			return (target, kinds::CONF_EXTERNAL);
		}
		let target = if fallback_kind == kinds::ANNOTATION_TYPE {
			extend_segment(&self.module, fallback_kind, name)
		} else {
			same_package_symbol_target(&self.module, name)
		};
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

pub(super) fn collect_import_tables(
	node: Node<'_>,
	source: &[u8],
	module: &Moniker,
	imports: &mut ImportConfidenceTable,
	import_targets: &mut ImportTargetTable,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() != "import_declaration" {
			collect_import_tables(child, source, module, imports, import_targets);
			continue;
		}
		let Some((pieces, confidence, wildcard)) = import_parts(child, source) else {
			continue;
		};
		if wildcard {
			continue;
		}
		let target = symbol_target(module, &pieces, confidence);
		if let Some(last) = pieces.last().copied() {
			let last_bytes = last.as_bytes().to_vec();
			imports.insert(last_bytes.clone(), confidence);
			import_targets.insert(last_bytes, target);
		}
	}
}

pub(super) fn collect_value_type_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	module: &Moniker,
	type_table: &HashMap<&'src [u8], Moniker>,
	import_targets: &ImportTargetTable,
	out: &mut ValueTypeTable,
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
				if child.kind() == "record_declaration" {
					collect_record_component_types(
						child,
						source,
						&scope,
						module,
						type_table,
						import_targets,
						out,
					);
				}
				if let Some(body) = child.child_by_field_name("body") {
					collect_value_type_table(
						body,
						source,
						&scope,
						module,
						type_table,
						import_targets,
						out,
					);
				}
			}
			"field_declaration" => {
				collect_field_types(
					child,
					source,
					parent,
					module,
					type_table,
					import_targets,
					out,
				);
			}
			_ => {
				collect_value_type_table(
					child,
					source,
					parent,
					module,
					type_table,
					import_targets,
					out,
				);
			}
		}
	}
}

fn collect_field_types<'src>(
	field: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	module: &Moniker,
	type_table: &HashMap<&'src [u8], Moniker>,
	import_targets: &ImportTargetTable,
	out: &mut ValueTypeTable,
) {
	let Some(type_node) = field.child_by_field_name("type") else {
		return;
	};
	let Some(type_name) = type_name_bytes(type_node, source) else {
		return;
	};
	let Some(type_target) =
		type_target_for_name(type_name.as_slice(), module, type_table, import_targets)
	else {
		return;
	};
	let mut cursor = field.walk();
	for child in field.children(&mut cursor) {
		if child.kind() != "variable_declarator" {
			continue;
		}
		let Some(name_node) = child.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, source);
		if !name.is_empty() {
			out.insert((parent.clone(), name.to_vec()), type_target.clone());
		}
	}
}

fn collect_record_component_types<'src>(
	record_node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	module: &Moniker,
	type_table: &HashMap<&'src [u8], Moniker>,
	import_targets: &ImportTargetTable,
	out: &mut ValueTypeTable,
) {
	let Some(params) = record_node.child_by_field_name("parameters") else {
		return;
	};
	let mut cursor = params.walk();
	for component in params.named_children(&mut cursor) {
		if !matches!(component.kind(), "formal_parameter" | "spread_parameter") {
			continue;
		}
		let Some(name_node) = component.child_by_field_name("name") else {
			continue;
		};
		let Some(type_node) = component.child_by_field_name("type") else {
			continue;
		};
		let Some(type_name) = type_name_bytes(type_node, source) else {
			continue;
		};
		let Some(type_target) =
			type_target_for_name(type_name.as_slice(), module, type_table, import_targets)
		else {
			continue;
		};
		let name = node_slice(name_node, source);
		if !name.is_empty() {
			out.insert((parent.clone(), name.to_vec()), type_target);
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
	out: &mut CallableTable,
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
				if child.kind() == "record_declaration" {
					collect_record_accessors(child, source, &scope, out);
				}
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
				out.insert((parent.clone(), name.to_vec(), slots.len()), seg);
			}
			_ => {
				collect_callable_table(child, source, parent, out);
			}
		}
	}
}

pub(super) fn collect_return_type_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	module: &Moniker,
	type_table: &HashMap<&'src [u8], Moniker>,
	import_targets: &ImportTargetTable,
	out: &mut ReturnTypeTable,
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
				if child.kind() == "record_declaration" {
					collect_record_accessor_return_types(
						child,
						source,
						&scope,
						module,
						type_table,
						import_targets,
						out,
					);
				}
				if let Some(body) = child.child_by_field_name("body") {
					collect_return_type_table(
						body,
						source,
						&scope,
						module,
						type_table,
						import_targets,
						out,
					);
				}
			}
			"method_declaration" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let Some(type_node) = child.child_by_field_name("type") else {
					continue;
				};
				let Some(type_name) = type_name_bytes(type_node, source) else {
					continue;
				};
				let Some(type_target) =
					type_target_for_name(type_name.as_slice(), module, type_table, import_targets)
				else {
					continue;
				};
				let name = node_slice(name_node, source);
				let arity = formal_parameter_slots(child, source).len();
				insert_return_type_aliases(out, parent, module, name, arity, type_target);
			}
			_ => {
				collect_return_type_table(
					child,
					source,
					parent,
					module,
					type_table,
					import_targets,
					out,
				);
			}
		}
	}
}

fn collect_record_accessor_return_types<'src>(
	record_node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	module: &Moniker,
	type_table: &HashMap<&'src [u8], Moniker>,
	import_targets: &ImportTargetTable,
	out: &mut ReturnTypeTable,
) {
	let Some(params) = record_node.child_by_field_name("parameters") else {
		return;
	};
	let explicit = explicit_no_arg_methods(record_node, source);
	let mut cursor = params.walk();
	for component in params.named_children(&mut cursor) {
		if !matches!(component.kind(), "formal_parameter" | "spread_parameter") {
			continue;
		}
		let Some(name_node) = component.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, source);
		if name.is_empty() || explicit.contains(name) {
			continue;
		}
		let Some(type_node) = component.child_by_field_name("type") else {
			continue;
		};
		let Some(type_name) = type_name_bytes(type_node, source) else {
			continue;
		};
		let Some(type_target) =
			type_target_for_name(type_name.as_slice(), module, type_table, import_targets)
		else {
			continue;
		};
		insert_return_type_aliases(out, parent, module, name, 0, type_target);
	}
}

fn insert_return_type_aliases(
	out: &mut ReturnTypeTable,
	owner: &Moniker,
	module: &Moniker,
	name: &[u8],
	arity: usize,
	type_target: Moniker,
) {
	out.insert((owner.clone(), name.to_vec(), arity), type_target.clone());
	if let Some(alias) = same_package_type_alias(module, owner) {
		out.insert((alias, name.to_vec(), arity), type_target);
	}
}

fn same_package_type_alias(module: &Moniker, owner: &Moniker) -> Option<Moniker> {
	let segment = owner.as_view().segments().last()?;
	is_java_type_kind(segment.kind).then(|| same_package_symbol_target(module, segment.name))
}

fn is_java_type_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::CLASS | kinds::INTERFACE | kinds::RECORD | kinds::ENUM | kinds::ANNOTATION_TYPE
	)
}

fn collect_record_accessors<'src>(
	record_node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	out: &mut CallableTable,
) {
	let Some(params) = record_node.child_by_field_name("parameters") else {
		return;
	};
	let explicit = explicit_no_arg_methods(record_node, source);
	let mut cursor = params.walk();
	for component in params.named_children(&mut cursor) {
		if !matches!(component.kind(), "formal_parameter" | "spread_parameter") {
			continue;
		}
		let Some(name_node) = component.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, source);
		if name.is_empty() || explicit.contains(name) {
			continue;
		}
		let seg = callable_segment_slots(name, &[]);
		out.insert((parent.clone(), name.to_vec(), 0), seg);
	}
}

fn argument_count(args: Node<'_>) -> usize {
	let mut cursor = args.walk();
	args.named_children(&mut cursor).count()
}

fn extend_arity_call(parent: &Moniker, kind: &[u8], name: &[u8], arity: usize) -> Moniker {
	let slots = vec![CallableSlot::default(); arity];
	extend_callable_slots(parent, kind, name, &slots)
}

fn type_target_for_name(
	name: &[u8],
	module: &Moniker,
	type_table: &HashMap<&[u8], Moniker>,
	import_targets: &ImportTargetTable,
) -> Option<Moniker> {
	if name.is_empty() || builtins::is_primitive_type(name) {
		return None;
	}
	if let Some(target) = type_table.get(name) {
		return Some(target.clone());
	}
	if let Some(target) = import_targets.get(name) {
		return Some(target.clone());
	}
	if builtins::is_java_lang_type(name) {
		let target = symbol_target(
			module,
			&["java", "lang", std::str::from_utf8(name).unwrap_or("")],
			kinds::CONF_EXTERNAL,
		);
		return Some(target);
	}
	Some(same_package_symbol_target(module, name))
}

fn type_name_bytes(node: Node<'_>, source: &[u8]) -> Option<Vec<u8>> {
	match node.kind() {
		"type_identifier" => Some(node_slice(node, source).to_vec()),
		"scoped_type_identifier" => Some(last_identifier(node, source).as_bytes().to_vec()),
		"generic_type" => Some(generic_type_short(node, source).as_bytes().to_vec()),
		"array_type" => node
			.child_by_field_name("element")
			.and_then(|element| type_name_bytes(element, source)),
		_ => None,
	}
}

fn explicit_no_arg_methods(record_node: Node<'_>, source: &[u8]) -> HashSet<Vec<u8>> {
	let mut out = HashSet::new();
	let Some(body) = record_node.child_by_field_name("body") else {
		return out;
	};
	let mut cursor = body.walk();
	for child in body.named_children(&mut cursor) {
		if child.kind() != "method_declaration" || !formal_parameter_slots(child, source).is_empty()
		{
			continue;
		}
		let Some(name_node) = child.child_by_field_name("name") else {
			continue;
		};
		let name = node_slice(name_node, source);
		if !name.is_empty() {
			out.insert(name.to_vec());
		}
	}
	out
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
		"type_identifier" => node_slice(obj, source),
		"scoped_type_identifier" => last_identifier(obj, source).as_bytes(),
		"generic_type" => generic_type_short(obj, source).as_bytes(),
		"method_invocation" => HINT_CALL,
		"field_access" => HINT_MEMBER,
		"scoped_identifier" => HINT_MEMBER,
		_ => b"",
	}
}

fn same_node(left: Node<'_>, right: Node<'_>) -> bool {
	left.start_byte() == right.start_byte()
		&& left.end_byte() == right.end_byte()
		&& left.kind_id() == right.kind_id()
}

fn import_parts<'a>(
	node: Node<'a>,
	source: &'a [u8],
) -> Option<(Vec<&'a str>, &'static [u8], bool)> {
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
	let path_node = path_node?;
	let dotted_bytes = node_slice(path_node, source);
	let dotted = std::str::from_utf8(dotted_bytes).unwrap_or("");
	if dotted.is_empty() {
		return None;
	}
	let pieces: Vec<&str> = dotted.split('.').collect();
	let confidence = external_or_imported(&pieces);
	Some((pieces, confidence, wildcard))
}

fn java_external_target_shape(target: &Moniker) -> bool {
	target
		.as_view()
		.segments()
		.any(|segment| segment.kind == kinds::EXTERNAL_PKG)
}

fn same_package_target_shape(module: &Moniker, target: &Moniker) -> bool {
	let module_segments: Vec<_> = module.as_view().segments().collect();
	let target_segments: Vec<_> = target.as_view().segments().collect();
	let Some(module_idx) = module_segments
		.iter()
		.position(|segment| segment.kind == kinds::MODULE)
	else {
		return false;
	};
	let Some(target_idx) = target_segments
		.iter()
		.position(|segment| segment.kind == kinds::MODULE)
	else {
		return false;
	};
	module_segments[..module_idx] == target_segments[..target_idx]
}

fn same_package_symbol_target(module: &Moniker, name: &[u8]) -> Moniker {
	let view = module.as_view();
	let mut b = MonikerBuilder::new();
	b.project(view.project());
	for segment in view.segments() {
		if segment.kind == kinds::MODULE {
			break;
		}
		b.segment(segment.kind, segment.name);
	}
	b.segment(kinds::MODULE, name);
	b.segment(kinds::PATH, name);
	b.build()
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

fn project_regime_builder(module: &Moniker) -> MonikerBuilder {
	let view = module.as_view();
	let mut b = MonikerBuilder::new();
	b.project(view.project());
	for segment in view.segments() {
		if segment.kind == crate::lang::kinds::LANG {
			break;
		}
		b.segment(segment.kind, segment.name);
	}
	b
}

fn wildcard_target(module: &Moniker, pieces: &[&str], confidence: &[u8]) -> Moniker {
	if confidence == kinds::CONF_IMPORTED && !pieces.is_empty() {
		let mut b = project_regime_builder(module);
		b.segment(crate::lang::kinds::LANG, b"java");
		for piece in pieces {
			b.segment(kinds::PACKAGE, piece.as_bytes());
		}
		return b.build();
	}
	external_package_target(module.as_view().project(), pieces)
}

fn symbol_target(module: &Moniker, pieces: &[&str], confidence: &[u8]) -> Moniker {
	if confidence == kinds::CONF_IMPORTED && !pieces.is_empty() {
		let mut b = project_regime_builder(module);
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
	external_package_target(module.as_view().project(), pieces)
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
