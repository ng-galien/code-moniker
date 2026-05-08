
use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::Moniker;

use super::canonicalize::{extend_callable_arity, extend_callable_typed, extend_segment, node_position};
use super::kinds;
use super::walker::Walker;

impl<'src> Walker<'src> {

	pub(super) fn handle_call(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let pos = node_position(node);
		let arity = call_argument_count(node);
		let Some(fn_node) = node.child_by_field_name("function") else {
			self.walk(node, scope, graph);
			return;
		};

		match fn_node.kind() {
			"identifier" => {
				let name = self.text_of(fn_node);
				let target = self.calls_target(name, arity);
				let attrs = RefAttrs {
					confidence: self.name_confidence(name.as_bytes()),
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
				self.maybe_emit_di_register(node, fn_node, scope, graph, pos);
			}
			"member_expression" => {
				if let Some(prop) = fn_node.child_by_field_name("property") {
					let name = self.text_of(prop);
					if !name.is_empty() {
						let target = self.method_call_target(name, arity);
						let attrs = RefAttrs {
							receiver_hint: receiver_hint(fn_node),
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
					}
				}
				if let Some(obj) = fn_node.child_by_field_name("object") {
					self.dispatch(obj, scope, graph);
				}
			}
			_ => {}
		}

		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk(args, scope, graph);
		}
	}

	pub(super) fn handle_new(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let pos = node_position(node);
		if let Some(ctor) = node.child_by_field_name("constructor") {
			let name = match ctor.kind() {
				"identifier" | "type_identifier" => Some(self.text_of(ctor)),
				"member_expression" => ctor
					.child_by_field_name("property")
					.map(|p| self.text_of(p)),
				_ => None,
			};
			if let Some(n) = name {
				if !n.is_empty() {
					let target = self.instantiates_target(n);
					let _ = graph.add_ref_attrs(scope, target, kinds::INSTANTIATES, Some(pos), &NAME_MATCH_ATTRS);
				}
			}
		}
		self.walk(node, scope, graph);
	}

	fn maybe_emit_di_register(
		&self,
		call: Node<'_>,
		callee: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		pos: (u32, u32),
	) {
		let callee_name = self.text_of(callee);
		if !self
			.presets
			.di_register_callees
			.iter()
			.any(|p| p == callee_name)
		{
			return;
		}

		let Some(args) = call.child_by_field_name("arguments") else { return };
		let mut cursor = args.walk();
		let mut named = 0usize;
		let mut the_id: Option<Node<'_>> = None;
		let mut reject = false;
		for c in args.children(&mut cursor) {
			if !c.is_named() {
				continue;
			}
			named += 1;
			if c.kind() == "identifier" {
				the_id = Some(c);
			} else {
				reject = true;
				break;
			}
		}
		if reject || named != 1 {
			return;
		}
		let Some(id) = the_id else { return };
		let name = self.text_of(id);
		if name.is_empty() {
			return;
		}
		let target = self.instantiates_target(name);
		let _ = graph.add_ref_attrs(scope, target, kinds::DI_REGISTER, Some(pos), &NAME_MATCH_ATTRS);
	}


	pub(super) fn handle_class_heritage(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			let edge: &[u8] = match child.kind() {
				"extends_clause" => kinds::EXTENDS,
				"implements_clause" => kinds::IMPLEMENTS,
				_ => {
					self.dispatch(child, scope, graph);
					continue;
				}
			};
			self.emit_heritage_refs(child, scope, edge, graph);
		}
	}

	pub(super) fn emit_heritage_refs(
		&self,
		clause: Node<'_>,
		scope: &Moniker,
		edge: &[u8],
		graph: &mut CodeGraph,
	) {
		let mut cursor = clause.walk();
		for c in clause.children(&mut cursor) {
			let pos = node_position(c);
			let name_kind = if edge == kinds::IMPLEMENTS {
				kinds::INTERFACE
			} else {
				kinds::CLASS
			};
			let name_opt = match c.kind() {
				"identifier" | "type_identifier" => Some(self.text_of(c).to_string()),
				"member_expression" => c
					.child_by_field_name("property")
					.map(|p| self.text_of(p).to_string()),
				"generic_type" => generic_short(c, self.source_bytes),
				"nested_type_identifier" => nested_type_short(c, self.source_bytes),
				_ => None,
			};
			let Some(name) = name_opt else { continue };
			if name.is_empty() {
				continue;
			}
			let target = self.heritage_target(name_kind, &name);
			let _ = graph.add_ref_attrs(scope, target, edge, Some(pos), &NAME_MATCH_ATTRS);
		}
	}


	pub(super) fn handle_decorator(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let pos = node_position(node);
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"identifier" => {
					let name = self.text_of(c);
					if !name.is_empty() {
						let target = self.calls_target(name, 0);
						let _ = graph.add_ref_attrs(scope, target, kinds::ANNOTATES, Some(pos), &NAME_MATCH_ATTRS);
					}
				}
				"call_expression" => {
					if let Some(fn_node) = c.child_by_field_name("function") {
						if fn_node.kind() == "identifier" {
							let name = self.text_of(fn_node);
							let arity = call_argument_count(c);
							let target = self.calls_target(name, arity);
							let _ = graph.add_ref_attrs(scope, target, kinds::ANNOTATES, Some(pos), &NAME_MATCH_ATTRS);
						}
					}
					if let Some(args) = c.child_by_field_name("arguments") {
						self.walk(args, scope, graph);
					}
				}
				_ => {}
			}
		}
	}


	pub(super) fn emit_uses_type_recursive(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		match node.kind() {
			"type_identifier" => {
				let name = self.text_of(node);
				if name.is_empty() {
					return;
				}
				let target = self.heritage_target(kinds::CLASS, name);
				let _ = graph.add_ref_attrs(scope, target, kinds::USES_TYPE, Some(node_position(node)), &NAME_MATCH_ATTRS);
			}
			"nested_type_identifier" => {
				if let Some(name) = nested_type_short(node, self.source_bytes) {
					let target = self.heritage_target(kinds::CLASS, &name);
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::USES_TYPE,
						Some(node_position(node)),
						&NAME_MATCH_ATTRS,
					);
				}
			}
			"generic_type" => {
				if let Some(name) = generic_short(node, self.source_bytes) {
					let target = self.heritage_target(kinds::CLASS, &name);
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::USES_TYPE,
						Some(node_position(node)),
						&NAME_MATCH_ATTRS,
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


	pub(super) fn emit_reads_in_children(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "identifier" {
				self.emit_read_at(c, scope, graph);
			} else {
				self.dispatch(c, scope, graph);
			}
		}
	}

	pub(super) fn emit_read_at(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let name = self.text_of(node);
		if name.is_empty() {
			return;
		}
		let target = self.read_target(name);
		let attrs = RefAttrs {
			confidence: self.name_confidence(name.as_bytes()),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::READS, Some(node_position(node)), &attrs);
	}

	pub(super) fn handle_member_like(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if let Some(obj) = node.child_by_field_name("object") {
			if obj.kind() == "identifier" {
				self.emit_read_at(obj, scope, graph);
			} else {
				self.dispatch(obj, scope, graph);
			}
		}
	}

	pub(super) fn handle_binary_like(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		for field in &["left", "right"] {
			if let Some(c) = node.child_by_field_name(field) {
				if c.kind() == "identifier" {
					self.emit_read_at(c, scope, graph);
				} else {
					self.dispatch(c, scope, graph);
				}
			}
		}
	}

	pub(super) fn handle_unary_like(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if let Some(arg) = node.child_by_field_name("argument") {
			if arg.kind() == "identifier" {
				self.emit_read_at(arg, scope, graph);
			} else {
				self.dispatch(arg, scope, graph);
			}
		}
	}

	pub(super) fn handle_ternary(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		for field in &["condition", "consequence", "alternative"] {
			if let Some(c) = node.child_by_field_name(field) {
				if c.kind() == "identifier" {
					self.emit_read_at(c, scope, graph);
				} else {
					self.dispatch(c, scope, graph);
				}
			}
		}
	}

	pub(super) fn handle_jsx_element(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if let Some(name) = node.child_by_field_name("name") {
			if name.kind() == "identifier" {
				self.emit_read_at(name, scope, graph);
			}
		}
		self.walk(node, scope, graph);
	}


	fn calls_target(&self, name: &str, arity: u16) -> Moniker {
		extend_callable_arity(&self.module, kinds::FUNCTION, name.as_bytes(), arity)
	}

	fn method_call_target(&self, name: &str, arity: u16) -> Moniker {
		extend_callable_arity(&self.module, kinds::METHOD, name.as_bytes(), arity)
	}

	fn instantiates_target(&self, name: &str) -> Moniker {
		extend_segment(&self.module, kinds::CLASS, name.as_bytes())
	}

	fn heritage_target(&self, kind: &[u8], name: &str) -> Moniker {
		extend_segment(&self.module, kind, name.as_bytes())
	}

	fn read_target(&self, name: &str) -> Moniker {
		extend_callable_typed(&self.module, kinds::FUNCTION, name.as_bytes(), &[] as &[&[u8]])
	}
}

const NAME_MATCH_ATTRS: RefAttrs<'static> = RefAttrs {
	receiver_hint: b"",
	alias: b"",
	confidence: kinds::CONF_NAME_MATCH,
	binding: b"",
};

fn receiver_hint(member_expr: Node<'_>) -> &'static [u8] {
	let Some(obj) = member_expr.child_by_field_name("object") else {
		return b"";
	};
	match obj.kind() {
		"this" => b"this",
		"super" => b"super",
		"identifier" => b"identifier",
		"call_expression" => b"call",
		"member_expression" => b"member",
		"subscript_expression" => b"subscript",
		_ => b"",
	}
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
