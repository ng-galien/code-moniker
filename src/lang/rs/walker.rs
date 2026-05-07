//! AST traversal for tree-sitter-rust. Dispatches each top-level
//! item to its def emitter; `impl_item` re-parents members onto the
//! type being implemented.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use super::canonicalize::{
	closure_arity, extend, extend_method, function_arity, impl_type_name, node_position,
};
use super::kinds;

pub(super) struct Walker<'src> {
	pub(super) source_bytes: &'src [u8],
	pub(super) module: Moniker,
	/// Names of `mod foo;` / `mod foo {}` declared at file root. A bare
	/// `use foo::X;` where `foo` is in this set resolves as `self::foo::X`
	/// (project-local) rather than as an external crate. Without this,
	/// the codebase pattern `mod canonicalize; use canonicalize::X;`
	/// would mis-tag `canonicalize` as external.
	pub(super) local_mods: HashSet<String>,
	pub(super) deep: bool,
}

/// Pre-pass: collect names of every `mod_item` at file root. Nested
/// `mod` declarations are not tracked here — they are scoped to inner
/// modules and cannot match a top-level `use` argument.
pub(super) fn collect_local_mods(root: Node<'_>, source: &[u8]) -> HashSet<String> {
	let mut out = HashSet::new();
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() == "mod_item" {
			if let Some(name) = child.child_by_field_name("name") {
				if let Ok(s) = name.utf8_text(source) {
					out.insert(s.to_string());
				}
			}
		}
	}
	out
}

impl<'src> Walker<'src> {
	pub(super) fn walk(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"struct_item" => self.handle_simple_def(child, parent, graph, kinds::CLASS),
				"enum_item" => self.handle_simple_def(child, parent, graph, kinds::ENUM),
				"trait_item" => self.handle_simple_def(child, parent, graph, kinds::INTERFACE),
				"type_item" => {
					self.handle_simple_def(child, parent, graph, kinds::TYPE_ALIAS)
				}
				"function_item" => self.handle_function(child, parent, graph, kinds::FUNCTION),
				"impl_item" => self.handle_impl(child, parent, graph),
				"use_declaration" => self.handle_use(child, parent, graph),
				_ => {}
			}
		}
	}

	fn handle_simple_def(
		&self,
		node: Node<'_>,
		parent: &Moniker,
		graph: &mut CodeGraph,
		kind: &[u8],
	) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend(parent, kind, name.as_bytes());
		let _ = graph.add_def(m, kind, parent, Some(node_position(node)));
	}

	fn handle_function(
		&self,
		node: Node<'_>,
		parent: &Moniker,
		graph: &mut CodeGraph,
		kind: &[u8],
	) {
		let Some(name) = self.field_text(node, "name") else { return };
		let arity = function_arity(node, self.source_bytes);
		let m = extend_method(parent, kind, name.as_bytes(), arity);
		let _ = graph.add_def(m.clone(), kind, parent, Some(node_position(node)));
		if !self.deep {
			return;
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			self.emit_params(params, &m, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_callable_body(body, &m, graph);
		}
	}

	/// Handles both `parameters` (function_item: typed `parameter`
	/// wrappers) and `closure_parameters` (bare pattern children when
	/// untyped, e.g. `|x|`).
	fn emit_params(&self, params: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"self_parameter" => {
					self.emit_pattern_leaf(callable, kinds::PARAM, b"self", child, graph)
				}
				"parameter" => {
					if let Some(pattern) = child.child_by_field_name("pattern") {
						self.emit_pattern_defs(pattern, callable, kinds::PARAM, child, graph);
					}
				}
				_ => self.emit_pattern_defs(child, callable, kinds::PARAM, child, graph),
			}
		}
	}

	/// Walk a destructuring pattern, emitting one def of `kind` per
	/// identifier reached. `_` is silently skipped. Used for both
	/// `param` and `local` emission — they share the same pattern
	/// shapes (tuples, structs, references).
	fn emit_pattern_defs(
		&self,
		pattern: Node<'_>,
		callable: &Moniker,
		kind: &[u8],
		anchor: Node<'_>,
		graph: &mut CodeGraph,
	) {
		match pattern.kind() {
			"identifier" => {
				if let Ok(name) = pattern.utf8_text(self.source_bytes) {
					self.emit_pattern_leaf(callable, kind, name.as_bytes(), anchor, graph);
				}
			}
			"_" => {}
			_ => {
				let mut cursor = pattern.walk();
				for inner in pattern.named_children(&mut cursor) {
					self.emit_pattern_defs(inner, callable, kind, anchor, graph);
				}
			}
		}
	}

	fn emit_pattern_leaf(
		&self,
		callable: &Moniker,
		kind: &[u8],
		name: &[u8],
		anchor: Node<'_>,
		graph: &mut CodeGraph,
	) {
		let m = extend(callable, kind, name);
		let _ = graph.add_def(m, kind, callable, Some(node_position(anchor)));
	}

	/// Containment rule: every emitted def's parent is `callable`, not
	/// the syntactic block — locals inside `if cond { let x = … }`
	/// still attach to the enclosing function.
	fn walk_callable_body(&self, node: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			match child.kind() {
				"let_declaration" => self.handle_let(child, callable, graph),
				// `expression_statement` wraps standalone control-flow used as a
				// statement (`if cond { let x = ... }` without a trailing `;`),
				// so dropping it here would silently lose locals nested in
				// statement-position blocks.
				"block" | "if_expression" | "match_expression" | "while_expression"
				| "for_expression" | "loop_expression" | "match_arm" | "match_block"
				| "expression_statement" => {
					self.walk_callable_body(child, callable, graph);
				}
				_ => {}
			}
		}
	}

	fn handle_let(&self, node: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		let Some(pattern) = node.child_by_field_name("pattern") else { return };
		self.emit_pattern_defs(pattern, callable, kinds::LOCAL, node, graph);
		if let Some(value) = node.child_by_field_name("value") {
			if value.kind() == "closure_expression" {
				if let Some(bind_name) = first_identifier(pattern, self.source_bytes) {
					self.emit_named_closure(value, callable, bind_name.as_bytes(), graph);
				}
			}
		}
	}

	fn emit_named_closure(
		&self,
		closure: Node<'_>,
		callable: &Moniker,
		name: &[u8],
		graph: &mut CodeGraph,
	) {
		let arity = closure_arity(closure);
		let m = extend_method(callable, kinds::FUNCTION, name, arity);
		let _ = graph.add_def(
			m.clone(),
			kinds::FUNCTION,
			callable,
			Some(node_position(closure)),
		);
		if let Some(params) = closure.child_by_field_name("parameters") {
			self.emit_params(params, &m, graph);
		}
		if let Some(body) = closure.child_by_field_name("body") {
			self.walk_callable_body(body, &m, graph);
		}
	}

	/// `impl Foo { fn bar() {} }` re-parents `bar` to `Foo`. The impl
	/// block itself is NOT a def — it's a scoping mechanism, per the
	/// canonicalization contract.
	///
	/// `impl Trait for Foo { ... }` additionally emits an `implements`
	/// ref from `Foo` → `Trait` (handled by `handle_impl_trait_for` in
	/// `refs.rs`).
	fn handle_impl(&self, node: Node<'_>, _parent: &Moniker, graph: &mut CodeGraph) {
		let Some(type_node) = node.child_by_field_name("type") else { return };
		let Some(type_name) = impl_type_name(type_node, self.source_bytes) else { return };
		// Members of `impl Foo` land under the local `class:Foo` moniker.
		// If `Foo` is not defined in this module, this synthesizes a
		// moniker; the members are still attached even though the type
		// def itself may live elsewhere — `code_graph @> moniker` will
		// flag it.
		let type_moniker = extend(&self.module, kinds::CLASS, type_name.as_bytes());
		self.handle_impl_trait_for(node, &type_moniker, graph);
		let Some(body) = node.child_by_field_name("body") else { return };
		self.walk_impl_body(body, &type_moniker, graph);
	}

	fn walk_impl_body(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"function_item" => self.handle_function(child, parent, graph, kinds::METHOD),
				_ => {}
			}
		}
	}

	pub(super) fn field_text(&self, node: Node<'_>, field: &str) -> Option<&'src str> {
		node.child_by_field_name(field)?
			.utf8_text(self.source_bytes)
			.ok()
	}
}

/// Recursively find the first `identifier` node inside a pattern. Used
/// to pick a name for a `let f = |...| ...` closure binding when the
/// pattern destructures.
fn first_identifier<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
	if node.kind() == "identifier" {
		return node.utf8_text(source).ok();
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if let Some(found) = first_identifier(child, source) {
			return Some(found);
		}
	}
	None
}

