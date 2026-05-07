//! AST traversal: dispatch tree-sitter nodes to the appropriate
//! def emitter (class, method, function) or to the refs module.

use tree_sitter::Node;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use super::canonicalize::{extend_method, extend_segment, node_position};
use super::kinds;

pub(super) struct Walker<'src> {
	pub(super) source_bytes: &'src [u8],
	pub(super) module: Moniker,
}

impl<'src> Walker<'src> {
	pub(super) fn walk(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"class_declaration" => self.handle_class(child, parent, graph),
				"function_declaration" => self.handle_function(child, parent, graph),
				"import_statement" => self.handle_import(child, parent, graph),
				"export_statement" => self.walk(child, parent, graph),
				_ => {}
			}
		}
	}

	fn handle_class(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let class_moniker = extend_segment(parent, kinds::CLASS, name.as_bytes());
		let _ = graph.add_def(
			class_moniker.clone(),
			kinds::CLASS,
			parent,
			Some(node_position(node)),
		);
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_class_body(body, &class_moniker, graph);
		}
	}

	fn walk_class_body(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() == "method_definition" {
				self.handle_method(child, parent, graph);
			}
		}
	}

	fn handle_method(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_method(parent, kinds::METHOD, name.as_bytes(), 0);
		let _ = graph.add_def(m, kinds::METHOD, parent, Some(node_position(node)));
	}

	fn handle_function(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_method(parent, kinds::FUNCTION, name.as_bytes(), 0);
		let _ = graph.add_def(m, kinds::FUNCTION, parent, Some(node_position(node)));
	}

	pub(super) fn field_text(&self, node: Node<'_>, field: &str) -> Option<&'src str> {
		node.child_by_field_name(field)?
			.utf8_text(self.source_bytes)
			.ok()
	}
}
