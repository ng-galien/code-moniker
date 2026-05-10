use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs};
use crate::core::moniker::Moniker;

use super::canonicalize::{
	extend_callable_typed, extend_segment, extend_segment_u32, find_named_child, node_position,
	parameter_list_types, parameter_types,
};
use super::kinds;
use super::scope::{is_callable_scope, modifier_visibility};

#[derive(Clone, Debug)]
pub(super) struct ImportEntry {
	pub confidence: &'static [u8],
	pub module_prefix: Moniker,
}

pub(super) struct Walker<'src> {
	pub(super) source_bytes: &'src [u8],
	pub(super) module: Moniker,
	pub(super) deep: bool,
	pub(super) local_scope: RefCell<Vec<HashSet<&'src [u8]>>>,
	pub(super) imports: RefCell<HashMap<&'src [u8], ImportEntry>>,
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
			"namespace_declaration" | "file_scoped_namespace_declaration" => {
				self.walk(node, scope, graph);
			}
			"comment" => self.handle_comment(node, scope, graph),
			"class_declaration" => self.handle_type(node, scope, graph, kinds::CLASS),
			"struct_declaration" => self.handle_type(node, scope, graph, kinds::STRUCT),
			"interface_declaration" => self.handle_type(node, scope, graph, kinds::INTERFACE),
			"enum_declaration" => self.handle_type(node, scope, graph, kinds::ENUM),
			"record_declaration" => self.handle_record(node, scope, graph, kinds::RECORD),
			"record_struct_declaration" => self.handle_record(node, scope, graph, kinds::STRUCT),
			"method_declaration" => self.handle_method(node, scope, graph),
			"constructor_declaration" => self.handle_constructor(node, scope, graph),
			"field_declaration" => self.handle_field(node, scope, graph),
			"property_declaration" => self.handle_property(node, scope, graph),
			"using_directive" => self.handle_using(node, scope, graph),
			"invocation_expression" => self.handle_invocation(node, scope, graph),
			"object_creation_expression" => self.handle_object_creation(node, scope, graph),
			"local_declaration_statement" => self.handle_local_declaration(node, scope, graph),
			"foreach_statement" => self.handle_foreach(node, scope, graph),
			_ => self.walk(node, scope, graph),
		}
	}

	fn handle_comment(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let m = extend_segment_u32(scope, kinds::COMMENT, node.start_byte() as u32);
		let _ = graph.add_def(m, kinds::COMMENT, scope, Some(node_position(node)));
	}

	fn handle_type(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph, kind: &[u8]) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(scope, kind, name.as_bytes());
		let default_vis = if scope == &self.module {
			kinds::VIS_PACKAGE
		} else {
			kinds::VIS_PRIVATE
		};
		let attrs = DefAttrs {
			visibility: modifier_visibility(node, default_vis),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(m.clone(), kind, scope, Some(node_position(node)), &attrs);
		self.emit_annotations_from(node, &m, graph);
		if let Some(bases) = find_named_child(node, "base_list") {
			self.emit_base_list(bases, &m, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
	}

	fn handle_record(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph, kind: &[u8]) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let m = extend_segment(scope, kind, name.as_bytes());
		let default_vis = if scope == &self.module {
			kinds::VIS_PACKAGE
		} else {
			kinds::VIS_PRIVATE
		};
		let attrs = DefAttrs {
			visibility: modifier_visibility(node, default_vis),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(m.clone(), kind, scope, Some(node_position(node)), &attrs);
		if let Some(plist) = find_named_child(node, "parameter_list") {
			let types = parameter_list_types(plist, self.source_bytes);
			let signature = crate::lang::callable::join_bytes_with_comma(&types);
			let ctor = extend_callable_typed(&m, kinds::CONSTRUCTOR, name.as_bytes(), &types);
			let ctor_attrs = DefAttrs {
				visibility: kinds::VIS_PUBLIC,
				signature: &signature,
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(
				ctor,
				kinds::CONSTRUCTOR,
				&m,
				Some(node_position(node)),
				&ctor_attrs,
			);
		}
		if let Some(body) = find_named_child(node, "declaration_list") {
			self.walk(body, &m, graph);
		}
	}

	fn handle_method(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		self.emit_callable(node, name.as_bytes(), kinds::METHOD, scope, graph);
	}

	fn handle_constructor(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		self.emit_callable(node, name.as_bytes(), kinds::CONSTRUCTOR, scope, graph);
	}

	fn emit_callable(
		&self,
		node: Node<'_>,
		name: &[u8],
		kind: &[u8],
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let types = parameter_types(node, self.source_bytes);
		let signature = crate::lang::callable::join_bytes_with_comma(&types);
		let m = extend_callable_typed(scope, kind, name, &types);
		let attrs = DefAttrs {
			visibility: modifier_visibility(node, kinds::VIS_PRIVATE),
			signature: &signature,
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(m.clone(), kind, scope, Some(node_position(node)), &attrs);
		self.emit_annotations_from(node, &m, graph);
		self.push_local_scope();
		self.bind_parameter_names(node, &m, graph);
		self.emit_callable_type_refs(node, &m, graph);
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
		self.pop_local_scope();
	}

	fn emit_callable_type_refs(&self, callable: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(rt) = callable.child_by_field_name("returns") {
			self.emit_uses_type(rt, scope, graph);
		}
		if let Some(params) = callable.child_by_field_name("parameters") {
			let mut cursor = params.walk();
			for p in params.named_children(&mut cursor) {
				if p.kind() != "parameter" {
					continue;
				}
				if let Some(t) = p.child_by_field_name("type") {
					self.emit_uses_type(t, scope, graph);
				}
			}
		}
	}

	fn handle_field(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let visibility = modifier_visibility(node, kinds::VIS_PRIVATE);
		self.emit_annotations_from(node, scope, graph);
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
			let Ok(s) = name_node.utf8_text(self.source_bytes) else {
				continue;
			};
			let m = extend_segment(scope, kinds::FIELD, s.as_bytes());
			let attrs = DefAttrs {
				visibility,
				..DefAttrs::default()
			};
			let _ = graph.add_def_attrs(m, kinds::FIELD, scope, Some(node_position(child)), &attrs);
		}
	}

	fn handle_property(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else {
			return;
		};
		let visibility = modifier_visibility(node, kinds::VIS_PRIVATE);
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let m = extend_segment(scope, kinds::PROPERTY, name.as_bytes());
		let attrs = DefAttrs {
			visibility,
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::PROPERTY,
			scope,
			Some(node_position(node)),
			&attrs,
		);
		self.emit_annotations_from(node, &m, graph);
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
			if in_callable
				&& let Some(name_node) = child.child_by_field_name("name")
				&& let Ok(s) = name_node.utf8_text(self.source_bytes)
				&& !s.is_empty()
				&& s != "_"
			{
				self.record_local(s.as_bytes());
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, s.as_bytes());
					let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(name_node)));
				}
			}
			let mut dc = child.walk();
			for c in child.named_children(&mut dc) {
				if c.kind() != "identifier" {
					self.dispatch(c, scope, graph);
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
			&& let Ok(s) = left.utf8_text(self.source_bytes)
			&& !s.is_empty()
			&& s != "_"
		{
			self.record_local(s.as_bytes());
			if self.deep {
				let m = extend_segment(scope, kinds::LOCAL, s.as_bytes());
				let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(left)));
			}
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.dispatch(right, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, scope, graph);
		}
	}

	fn bind_parameter_names(
		&self,
		callable: Node<'_>,
		callable_m: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(params) = callable.child_by_field_name("parameters") else {
			return;
		};
		let mut cursor = params.walk();
		for p in params.named_children(&mut cursor) {
			if p.kind() != "parameter" {
				continue;
			}
			let Some(name_node) = p.child_by_field_name("name") else {
				continue;
			};
			let Ok(s) = name_node.utf8_text(self.source_bytes) else {
				continue;
			};
			if s.is_empty() || s == "_" {
				continue;
			}
			self.record_local(s.as_bytes());
			if self.deep {
				let m = extend_segment(callable_m, kinds::PARAM, s.as_bytes());
				let _ = graph.add_def(m, kinds::PARAM, callable_m, Some(node_position(name_node)));
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
	root: Node<'_>,
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
