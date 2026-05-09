use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{extend_callable_arity, extend_segment, node_position};
use super::kinds;
use super::walker::{ImportEntry, Walker};

impl<'src> Walker<'src> {
	pub(super) fn emit_uses_type(
		&self,
		type_node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		self.emit_type_ref(type_node, scope, kinds::USES_TYPE, graph);
	}

	pub(super) fn emit_base_list(
		&self,
		base_list: Node<'_>,
		owner: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = base_list.walk();
		for entry in base_list.named_children(&mut cursor) {
			self.emit_type_ref(entry, owner, kinds::EXTENDS, graph);
		}
	}

	fn resolve_type_node(&self, type_node: Node<'_>) -> Option<(Moniker, &'static [u8])> {
		match type_node.kind() {
			"identifier" => {
				let name = self.text_of(type_node);
				if name.is_empty() {
					return None;
				}
				Some(self.resolve_type_target(name.as_bytes(), kinds::CLASS))
			}
			"qualified_name" => {
				let leaf = qualified_leaf_identifier(type_node)?;
				let name = self.text_of(leaf);
				if name.is_empty() {
					return None;
				}
				Some(self.resolve_type_target(name.as_bytes(), kinds::CLASS))
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

	fn emit_type_ref(
		&self,
		type_node: Node<'_>,
		scope: &Moniker,
		ref_kind: &[u8],
		graph: &mut CodeGraph,
	) {
		match type_node.kind() {
			"predefined_type" => {}
			"identifier" | "qualified_name" => {
				self.emit_resolved_type_ref(
					type_node,
					scope,
					ref_kind,
					node_position(type_node),
					graph,
				);
			}
			"generic_name" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					match c.kind() {
						"identifier" => {
							self.emit_resolved_type_ref(
								c,
								scope,
								ref_kind,
								node_position(c),
								graph,
							);
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

	pub(super) fn handle_using(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
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
		let pieces = self.qualified_pieces(path_node);
		if pieces.is_empty() {
			return;
		}
		let confidence = stdlib_or_imported(&pieces);
		let alias = alias_node.map(|n| self.text_of(n)).unwrap_or("");
		let bind_name: &'src str = if !alias.is_empty() {
			alias
		} else {
			pieces.last().copied().unwrap_or("")
		};

		let module_prefix = build_module_target(self.module.as_view().project(), &pieces);
		if !bind_name.is_empty() {
			self.imports.borrow_mut().insert(
				bind_name.as_bytes(),
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

	fn qualified_pieces(&self, node: Node<'_>) -> Vec<&'src str> {
		let mut out = Vec::new();
		collect_qualified(node, self.source_bytes, &mut out);
		out
	}

	pub(super) fn handle_invocation(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let arity = argument_count(node);
		let Some(callee) = node.child_by_field_name("function") else {
			self.walk(node, scope, graph);
			return;
		};
		match callee.kind() {
			"identifier" => self.emit_simple_call(callee, scope, arity, pos, graph),
			"member_access_expression" => {
				self.emit_member_call(callee, scope, arity, pos, graph);
			}
			_ => self.dispatch(callee, scope, graph),
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk(args, scope, graph);
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
		let name = self.text_of(callee);
		if name.is_empty() {
			return;
		}
		if let Some(entry) = self.import_entry_for(name.as_bytes()) {
			let target = extend_callable_arity(
				&entry.module_prefix,
				kinds::FUNCTION,
				name.as_bytes(),
				arity,
			);
			let attrs = RefAttrs {
				confidence: entry.confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
			return;
		}
		let Some(conf) = self.name_confidence(name.as_bytes()) else {
			return;
		};
		let target = if conf == kinds::CONF_LOCAL {
			extend_segment(scope, kinds::LOCAL, name.as_bytes())
		} else {
			extend_callable_arity(&self.module, kinds::FUNCTION, name.as_bytes(), arity)
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
			self.walk(callee, scope, graph);
			return;
		};
		let name = self.text_of(name_node);
		if name.is_empty() {
			return;
		}
		let operand = callee.child_by_field_name("expression");
		let target = extend_callable_arity(&self.module, kinds::METHOD, name.as_bytes(), arity);
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
			self.dispatch(op, scope, graph);
		}
	}

	pub(super) fn emit_annotations_from(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
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
				let name = self.text_of(leaf);
				if name.is_empty() {
					continue;
				}
				let (target, conf) = self.resolve_type_target(name.as_bytes(), kinds::CLASS);
				let attrs = RefAttrs {
					confidence: conf,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					scope,
					target,
					kinds::ANNOTATES,
					Some(node_position(attr)),
					&attrs,
				);
				if let Some(args) = attr.child_by_field_name("arguments") {
					self.walk(args, scope, graph);
				}
			}
		}
	}

	pub(super) fn handle_object_creation(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if let Some(type_node) = node.child_by_field_name("type") {
			self.emit_type_ref(type_node, scope, kinds::INSTANTIATES, graph);
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk(args, scope, graph);
		}
	}
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
