use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{extend_callable_arity, extend_segment, join_bytes_with_comma};
use crate::lang::strategy::{LangStrategy, NodeShape, RefSpec, Symbol};
use crate::lang::tree_util::{find_named_child, node_position, node_slice};

use super::canonicalize::{extend_callable_typed, parameter_list_types, parameter_types};
use super::kinds;

#[derive(Clone)]
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
			"namespace_declaration" | "file_scoped_namespace_declaration" => NodeShape::Recurse,
			"class_declaration" => self.classify_type(node, scope, source, kinds::CLASS),
			"struct_declaration" => self.classify_type(node, scope, source, kinds::STRUCT),
			"interface_declaration" => self.classify_type(node, scope, source, kinds::INTERFACE),
			"enum_declaration" => self.classify_type(node, scope, source, kinds::ENUM),
			"record_declaration" => self.classify_record(node, scope, source, kinds::RECORD),
			"record_struct_declaration" => self.classify_record(node, scope, source, kinds::STRUCT),
			"method_declaration" => self.classify_callable(node, scope, source, kinds::METHOD),
			"constructor_declaration" => {
				self.classify_callable(node, scope, source, kinds::CONSTRUCTOR)
			}
			"field_declaration" => {
				self.handle_field(node, scope, graph);
				NodeShape::Skip
			}
			"property_declaration" => self.classify_property(node, scope, source, graph),
			"using_directive" => {
				self.handle_using(node, scope, graph);
				NodeShape::Skip
			}
			"invocation_expression" => {
				self.handle_invocation(node, scope, graph);
				NodeShape::Skip
			}
			"object_creation_expression" => {
				self.handle_object_creation(node, scope, graph);
				NodeShape::Skip
			}
			"local_declaration_statement" => {
				self.handle_local_declaration(node, scope, graph);
				NodeShape::Skip
			}
			"foreach_statement" => {
				self.handle_foreach(node, scope, graph);
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
			if let Some(rt) = node.child_by_field_name("returns") {
				self.emit_uses_type(rt, moniker, graph);
			}
			if let Some(params) = node.child_by_field_name("parameters") {
				self.emit_param_defs_and_types(params, moniker, graph);
			}
			return;
		}
		if kind == kinds::RECORD || (kind == kinds::STRUCT && is_record_struct(node)) {
			self.emit_record_primary_constructor(node, moniker, graph);
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
		_source: &[u8],
		graph: &mut CodeGraph,
	) {
		if sym_kind != kinds::RECORD && !(sym_kind == kinds::STRUCT && is_record_struct(node)) {
			return;
		}
		if find_named_child(node, "declaration_list").is_none() {
			self.emit_record_primary_constructor(node, sym_moniker, graph);
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
		if let Some(bases) = find_named_child(node, "base_list") {
			self.collect_base_list_refs(bases, &mut annotated_by);
		}
		self.collect_attribute_refs(node, &mut annotated_by);

		let default_vis = if scope == &self.module {
			kinds::VIS_PACKAGE
		} else {
			kinds::VIS_PRIVATE
		};
		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility: modifier_visibility(node, default_vis),
			signature: None,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_record<'src>(
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
		if let Some(bases) = find_named_child(node, "base_list") {
			self.collect_base_list_refs(bases, &mut annotated_by);
		}
		self.collect_attribute_refs(node, &mut annotated_by);

		let default_vis = if scope == &self.module {
			kinds::VIS_PACKAGE
		} else {
			kinds::VIS_PRIVATE
		};
		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility: modifier_visibility(node, default_vis),
			signature: None,
			body: find_named_child(node, "declaration_list"),
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
		let types = parameter_types(node, source);
		let signature = join_bytes_with_comma(&types);
		let moniker = extend_callable_typed(scope, kind, name, &types);

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		self.collect_attribute_refs(node, &mut annotated_by);

		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			self.record_param_locals(params);
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility: modifier_visibility(node, kinds::VIS_PRIVATE),
			signature: Some(signature),
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_property<'src>(
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
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let moniker = extend_segment(scope, kinds::PROPERTY, name);

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		self.collect_attribute_refs(node, &mut annotated_by);

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::PROPERTY,
			visibility: modifier_visibility(node, kinds::VIS_PRIVATE),
			signature: None,
			body: None,
			position: node_position(node),
			annotated_by,
		})
	}

	fn emit_record_primary_constructor(
		&self,
		node: Node<'_>,
		record: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(plist) = find_named_child(node, "parameter_list") else {
			return;
		};
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let name = node_slice(name_node, self.source_bytes);
		let types = parameter_list_types(plist, self.source_bytes);
		let signature = join_bytes_with_comma(&types);
		let ctor = extend_callable_typed(record, kinds::CONSTRUCTOR, name, &types);
		let attrs = DefAttrs {
			visibility: kinds::VIS_PUBLIC,
			signature: &signature,
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			ctor,
			kinds::CONSTRUCTOR,
			record,
			Some(node_position(node)),
			&attrs,
		);
	}

	fn record_param_locals(&self, params: Node<'_>) {
		let mut cursor = params.walk();
		for p in params.named_children(&mut cursor) {
			if p.kind() != "parameter" {
				continue;
			}
			let Some(name_node) = p.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			if name.is_empty() || name == b"_" {
				continue;
			}
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
		for p in params.named_children(&mut cursor) {
			if p.kind() != "parameter" {
				continue;
			}
			if let Some(t) = p.child_by_field_name("type") {
				self.emit_uses_type(t, callable, graph);
			}
			let Some(name_node) = p.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			if name.is_empty() || name == b"_" {
				continue;
			}
			if self.deep {
				let m = extend_segment(callable, kinds::PARAM, name);
				let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(name_node)));
			}
		}
	}

	fn handle_field(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let visibility = modifier_visibility(node, kinds::VIS_PRIVATE);
		self.emit_attribute_refs_on(node, scope, graph);
		let Some(decl) = find_named_child(node, "variable_declaration") else {
			return;
		};
		if let Some(t) = decl.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let mut cursor = decl.walk();
		for child in decl.named_children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			let m = extend_segment(scope, kinds::FIELD, name);
			let attrs = DefAttrs {
				visibility,
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(m, kinds::FIELD, scope, Some(node_position(child)), &attrs);
		}
	}

	fn handle_local_declaration(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(decl) = find_named_child(node, "variable_declaration") else {
			return;
		};
		if let Some(t) = decl.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let in_callable = is_callable_scope(scope, &self.module);
		let mut cursor = decl.walk();
		for child in decl.named_children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			if in_callable && let Some(name_node) = child.child_by_field_name("name") {
				let name = node_slice(name_node, self.source_bytes);
				if !name.is_empty() && name != b"_" {
					self.record_local(name);
					if self.deep {
						let m = extend_segment(scope, kinds::LOCAL, name);
						let _ =
							graph.add_def(m, kinds::LOCAL, scope, Some(node_position(name_node)));
					}
				}
			}
			let mut dc = child.walk();
			for c in child.named_children(&mut dc) {
				if c.kind() != "identifier" {
					self.recurse_subtree(c, scope, graph);
				}
			}
		}
	}

	fn handle_foreach(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let in_callable = is_callable_scope(scope, &self.module);
		if in_callable
			&& let Some(left) = node.child_by_field_name("left")
			&& left.kind() == "identifier"
		{
			let name = node_slice(left, self.source_bytes);
			if !name.is_empty() && name != b"_" {
				self.record_local(name);
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, name);
					let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(left)));
				}
			}
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.recurse_subtree(right, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_children(body, scope, graph);
		}
	}

	fn handle_using(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let alias_node = node.child_by_field_name("name");
		let mut path_node: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if matches!(c.kind(), "qualified_name" | "identifier")
				&& Some(c.id()) != alias_node.map(|n| n.id())
			{
				path_node = Some(c);
			}
		}
		let Some(path_node) = path_node else { return };
		let pieces = collect_qualified_pieces(path_node, self.source_bytes);
		if pieces.is_empty() {
			return;
		}
		let confidence = stdlib_or_imported(&pieces);
		let alias = alias_node
			.and_then(|n| n.utf8_text(self.source_bytes).ok())
			.unwrap_or("");
		let bind_name = if !alias.is_empty() {
			alias
		} else {
			pieces.last().copied().unwrap_or("")
		};

		let module_prefix = build_module_target(self.module.as_view().project(), &pieces);
		if !bind_name.is_empty() {
			self.imports.borrow_mut().insert(
				bind_name.as_bytes().to_vec(),
				ImportEntry {
					confidence,
					module_prefix: module_prefix.clone(),
				},
			);
		}
		let attrs = RefAttrs {
			confidence,
			alias: alias.as_bytes(),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			module_prefix,
			kinds::IMPORTS_MODULE,
			Some(pos),
			&attrs,
		);
	}

	fn handle_invocation(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let arity = argument_count(node);
		let Some(callee) = node.child_by_field_name("function") else {
			self.walk_children(node, scope, graph);
			return;
		};
		match callee.kind() {
			"identifier" => self.emit_simple_call(callee, scope, arity, pos, graph),
			"member_access_expression" => {
				self.emit_member_call(callee, scope, arity, pos, graph);
			}
			_ => self.recurse_subtree(callee, scope, graph),
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk_children(args, scope, graph);
		}
	}

	fn emit_simple_call(
		&self,
		callee: Node<'_>,
		scope: &Moniker,
		arity: u16,
		pos: (u32, u32),
		graph: &mut CodeGraph,
	) {
		let name = node_slice(callee, self.source_bytes);
		if name.is_empty() {
			return;
		}
		if let Some(entry) = self.import_entry_for(name) {
			let target = extend_callable_arity(&entry.module_prefix, kinds::FUNCTION, name, arity);
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
			extend_callable_arity(&self.module, kinds::FUNCTION, name, arity)
		};
		let attrs = RefAttrs {
			confidence: conf,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
	}

	fn emit_member_call(
		&self,
		callee: Node<'_>,
		scope: &Moniker,
		arity: u16,
		pos: (u32, u32),
		graph: &mut CodeGraph,
	) {
		let Some(name_node) = callee.child_by_field_name("name") else {
			self.walk_children(callee, scope, graph);
			return;
		};
		let name = node_slice(name_node, self.source_bytes);
		if name.is_empty() {
			return;
		}
		let operand = callee.child_by_field_name("expression");
		let target = extend_callable_arity(&self.module, kinds::METHOD, name, arity);
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

	fn handle_object_creation(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(type_node) = node.child_by_field_name("type") {
			self.emit_type_ref(type_node, scope, kinds::INSTANTIATES, graph);
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk_children(args, scope, graph);
		}
	}

	fn emit_uses_type(&self, type_node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.emit_type_ref(type_node, scope, kinds::USES_TYPE, graph);
	}

	fn emit_type_ref(
		&self,
		type_node: Node<'_>,
		scope: &Moniker,
		ref_kind: &'static [u8],
		graph: &mut CodeGraph,
	) {
		match type_node.kind() {
			"predefined_type" | "implicit_type" => {}
			"identifier" | "qualified_name" => {
				if let Some((target, confidence)) = self.resolve_type_node(type_node) {
					let attrs = RefAttrs {
						confidence,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						ref_kind,
						Some(node_position(type_node)),
						&attrs,
					);
				}
			}
			"generic_name" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					match c.kind() {
						"identifier" => {
							if let Some((target, confidence)) = self.resolve_type_node(c) {
								let attrs = RefAttrs {
									confidence,
									..RefAttrs::default()
								};
								let _ = graph.add_ref_attrs(
									scope,
									target,
									ref_kind,
									Some(node_position(c)),
									&attrs,
								);
							}
						}
						"type_argument_list" => {
							let mut ac = c.walk();
							for arg in c.named_children(&mut ac) {
								self.emit_type_ref(arg, scope, kinds::USES_TYPE, graph);
							}
						}
						_ => {}
					}
				}
			}
			"array_type" | "nullable_type" | "pointer_type" => {
				if let Some(inner) = type_node.child_by_field_name("type") {
					self.emit_type_ref(inner, scope, ref_kind, graph);
				}
			}
			"tuple_type" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					if let Some(t) = c.child_by_field_name("type") {
						self.emit_type_ref(t, scope, ref_kind, graph);
					}
				}
			}
			_ => {}
		}
	}

	fn resolve_type_node(&self, type_node: Node<'_>) -> Option<(Moniker, &'static [u8])> {
		match type_node.kind() {
			"identifier" => {
				let name = node_slice(type_node, self.source_bytes);
				if name.is_empty() {
					return None;
				}
				Some(self.resolve_type_target(name, kinds::CLASS))
			}
			"qualified_name" => {
				let leaf = qualified_leaf_identifier(type_node)?;
				let name = node_slice(leaf, self.source_bytes);
				if name.is_empty() {
					return None;
				}
				Some(self.resolve_type_target(name, kinds::CLASS))
			}
			_ => None,
		}
	}

	fn collect_base_list_refs(&self, base_list: Node<'_>, out: &mut Vec<RefSpec>) {
		let mut cursor = base_list.walk();
		for entry in base_list.named_children(&mut cursor) {
			let (leaf_node, name) = match entry.kind() {
				"identifier" => {
					let n = node_slice(entry, self.source_bytes);
					(entry, n)
				}
				"qualified_name" => {
					let Some(leaf) = qualified_leaf_identifier(entry) else {
						continue;
					};
					(leaf, node_slice(leaf, self.source_bytes))
				}
				"generic_name" => {
					let Some(leaf) = first_identifier_child(entry) else {
						continue;
					};
					(leaf, node_slice(leaf, self.source_bytes))
				}
				_ => continue,
			};
			if name.is_empty() {
				continue;
			}
			let (target, confidence) = self.resolve_type_target(name, kinds::CLASS);
			out.push(RefSpec {
				kind: kinds::EXTENDS,
				target,
				confidence,
				position: node_position(leaf_node),
				receiver_hint: b"",
				alias: b"",
			});
		}
	}

	fn collect_attribute_refs(&self, node: Node<'_>, out: &mut Vec<RefSpec>) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "attribute_list" {
				continue;
			}
			let mut alc = child.walk();
			for attr in child.named_children(&mut alc) {
				if attr.kind() != "attribute" {
					continue;
				}
				let Some(name_node) = attr.child_by_field_name("name") else {
					continue;
				};
				let leaf = match name_node.kind() {
					"identifier" => Some(name_node),
					"qualified_name" => qualified_leaf_identifier(name_node),
					_ => None,
				};
				let Some(leaf) = leaf else { continue };
				let name = node_slice(leaf, self.source_bytes);
				if name.is_empty() {
					continue;
				}
				let (target, confidence) = self.resolve_type_target(name, kinds::CLASS);
				out.push(RefSpec {
					kind: kinds::ANNOTATES,
					target,
					confidence,
					position: node_position(attr),
					receiver_hint: b"",
					alias: b"",
				});
			}
		}
	}

	fn emit_attribute_refs_on(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut refs: Vec<RefSpec> = Vec::new();
		self.collect_attribute_refs(node, &mut refs);
		for r in refs {
			let attrs = RefAttrs {
				confidence: r.confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, r.target, r.kind, Some(r.position), &attrs);
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
}

pub(super) fn collect_type_table<'src>(
	root: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	out: &mut HashMap<&'src [u8], Moniker>,
) {
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		let kind: Option<&[u8]> = match child.kind() {
			"class_declaration" => Some(kinds::CLASS),
			"struct_declaration" => Some(kinds::STRUCT),
			"record_declaration" => Some(kinds::RECORD),
			"record_struct_declaration" => Some(kinds::STRUCT),
			"interface_declaration" => Some(kinds::INTERFACE),
			"enum_declaration" => Some(kinds::ENUM),
			_ => None,
		};
		if let Some(kind) = kind {
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let Ok(name) = name_node.utf8_text(source) else {
				continue;
			};
			let m = extend_segment(parent, kind, name.as_bytes());
			out.entry(name.as_bytes()).or_insert_with(|| m.clone());
			if let Some(body) = child
				.child_by_field_name("body")
				.or_else(|| find_named_child(child, "declaration_list"))
			{
				collect_type_table(body, source, &m, out);
			}
		} else {
			collect_type_table(child, source, parent, out);
		}
	}
}

fn modifier_visibility(node: Node<'_>, default: &'static [u8]) -> &'static [u8] {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() != "modifier" {
			continue;
		}
		let mut mc = child.walk();
		for kw in child.children(&mut mc) {
			match kw.kind() {
				"public" => return kinds::VIS_PUBLIC,
				"protected" => return kinds::VIS_PROTECTED,
				"private" => return kinds::VIS_PRIVATE,
				"internal" => return kinds::VIS_PACKAGE,
				_ => {}
			}
		}
	}
	default
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

fn is_record_struct(node: Node<'_>) -> bool {
	node.kind() == "record_struct_declaration"
}

fn collect_qualified_pieces<'src>(node: Node<'_>, source: &'src [u8]) -> Vec<&'src str> {
	let mut out = Vec::new();
	collect_qualified(node, source, &mut out);
	out
}

fn collect_qualified<'src>(node: Node<'_>, source: &'src [u8], out: &mut Vec<&'src str>) {
	match node.kind() {
		"identifier" => {
			if let Ok(s) = node.utf8_text(source)
				&& !s.is_empty()
			{
				out.push(s);
			}
		}
		"qualified_name" => {
			if let Some(q) = node.child_by_field_name("qualifier") {
				collect_qualified(q, source, out);
			}
			if let Some(name) = node.child_by_field_name("name") {
				collect_qualified(name, source, out);
			}
		}
		_ => {}
	}
}

fn qualified_leaf_identifier(node: Node<'_>) -> Option<Node<'_>> {
	let mut cursor = node.walk();
	let mut last = None;
	for c in node.named_children(&mut cursor) {
		if c.kind() == "identifier" {
			last = Some(c);
		}
	}
	last
}

fn first_identifier_child(node: Node<'_>) -> Option<Node<'_>> {
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.find(|c| c.kind() == "identifier")
}

fn argument_count(call: Node<'_>) -> u16 {
	let Some(args) = call.child_by_field_name("arguments") else {
		return 0;
	};
	let mut cursor = args.walk();
	let mut count: u16 = 0;
	for c in args.named_children(&mut cursor) {
		if c.kind() == "argument" {
			count = count.saturating_add(1);
		}
	}
	count
}

fn receiver_hint<'a>(obj: Node<'_>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SUBSCRIPT, HINT_THIS};
	match obj.kind() {
		"this_expression" => HINT_THIS,
		"identifier" => obj.utf8_text(source).unwrap_or("").as_bytes(),
		"member_access_expression" => HINT_MEMBER,
		"invocation_expression" => HINT_CALL,
		"element_access_expression" => HINT_SUBSCRIPT,
		_ => b"",
	}
}

fn build_module_target(project: &[u8], pieces: &[&str]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(project);
	if !pieces.is_empty() {
		b.segment(kinds::EXTERNAL_PKG, pieces[0].as_bytes());
		for p in &pieces[1..] {
			b.segment(kinds::PATH, p.as_bytes());
		}
	}
	b.build()
}

fn stdlib_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	match pieces[0] {
		"System" | "Microsoft" | "mscorlib" => kinds::CONF_EXTERNAL,
		_ => kinds::CONF_IMPORTED,
	}
}
