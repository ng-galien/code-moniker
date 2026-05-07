//! AST traversal for tree-sitter-rust. Dispatches each top-level
//! item to its def emitter; `impl_item` re-parents members onto the
//! type being implemented.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use super::canonicalize::{
	extend, extend_method, function_arity, impl_type_name, node_position,
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
		let _ = graph.add_def(m, kind, parent, Some(node_position(node)));
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
