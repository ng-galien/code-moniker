//! AST traversal for tree-sitter-java: dispatches each node to its
//! def emitter or to the refs module.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs};
use crate::core::moniker::Moniker;

use super::canonicalize::{extend_callable_typed, extend_segment, node_position};
use super::kinds;
use super::scope::{is_callable_scope, modifier_visibility, section_title};

pub(super) struct Walker<'src> {
	pub(super) source_bytes: &'src [u8],
	pub(super) module: Moniker,
	pub(super) deep: bool,
	#[allow(dead_code)]
	pub(super) presets: &'src super::Presets,
	pub(super) local_scope: RefCell<Vec<HashSet<&'src [u8]>>>,
	/// Short imported name → consumer-side confidence bucket. `import
	/// java.util.List` puts `List → external`; relative project imports
	/// put `Foo → imported`.
	pub(super) imports: RefCell<HashMap<&'src [u8], &'static [u8]>>,
	/// Short type name → full moniker for every type declared in this
	/// compilation unit (top-level + nested). Built before the walk so
	/// extends/implements/instantiates/uses_type/annotates emit
	/// `confidence: resolved` with a real target when the name matches.
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
			"line_comment" | "block_comment" => self.handle_comment(node, scope, graph),
			"package_declaration" => {} // handled at extract entry
			"import_declaration" => self.handle_import(node, scope, graph),
			"class_declaration" => self.handle_class(node, scope, graph),
			"interface_declaration" => self.handle_interface(node, scope, graph),
			"enum_declaration" => self.handle_enum(node, scope, graph),
			"record_declaration" => self.handle_record(node, scope, graph),
			"annotation_type_declaration" => self.handle_annotation_type(node, scope, graph),
			"method_declaration" => self.handle_method(node, scope, graph),
			"constructor_declaration" => self.handle_constructor(node, scope, graph),
			"field_declaration" => self.handle_field(node, scope, graph),
			"local_variable_declaration" => self.handle_local_variable(node, scope, graph),
			"catch_formal_parameter" => self.handle_catch_param(node, scope, graph),
			"enhanced_for_statement" => self.handle_enhanced_for(node, scope, graph),
			"lambda_expression" => self.handle_lambda(node, scope, graph),
			"method_invocation" => self.handle_method_invocation(node, scope, graph),
			"object_creation_expression" => self.handle_object_creation(node, scope, graph),
			"marker_annotation" | "annotation" => self.handle_annotation(node, scope, graph),
			"identifier" => self.handle_identifier(node, scope, graph),
			"type_identifier" | "scoped_type_identifier" | "generic_type" => {
				self.emit_uses_type(node, scope, graph)
			}
			_ => self.walk(node, scope, graph),
		}
	}

	// --- comments / sections ---------------------------------------------

	fn handle_comment(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if let Some(title) = section_title(node, self.source_bytes) {
			let m = extend_segment(scope, kinds::SECTION, title.as_bytes());
			let _ = graph.add_def(m, kinds::SECTION, scope, Some(node_position(node)));
		}
	}

	// --- type-like declarations ------------------------------------------

	fn handle_class(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.handle_type_decl(node, scope, graph, kinds::CLASS);
	}

	fn handle_interface(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.handle_type_decl(node, scope, graph, kinds::INTERFACE);
	}

	fn handle_enum(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_segment(scope, kinds::ENUM, name.as_bytes());
		let attrs = DefAttrs {
			visibility: modifier_visibility(node),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::ENUM,
			scope,
			Some(node_position(node)),
			&attrs,
		);
		self.walk_type_body_with_enum_constants(node, &m, graph);
	}

	fn handle_record(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.handle_type_decl(node, scope, graph, kinds::RECORD);
	}

	fn handle_annotation_type(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		self.handle_type_decl(node, scope, graph, kinds::ANNOTATION_TYPE);
	}

	fn handle_type_decl(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		kind: &[u8],
	) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_segment(scope, kind, name.as_bytes());
		let attrs = DefAttrs {
			visibility: modifier_visibility(node),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kind,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		// heritage refs
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"superclass" => self.emit_heritage_refs(child, &m, kinds::EXTENDS, graph),
				"super_interfaces" | "extends_interfaces" => {
					self.emit_heritage_refs(child, &m, kinds::IMPLEMENTS, graph)
				}
				_ => {}
			}
		}

		// annotations on the declaration
		self.emit_annotations_from(node, &m, graph);

		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
	}

	fn walk_type_body_with_enum_constants(
		&self,
		enum_node: Node<'_>,
		parent: &Moniker,
		graph: &mut CodeGraph,
	) {
		// annotations + interfaces
		let mut cursor = enum_node.walk();
		for child in enum_node.children(&mut cursor) {
			if child.kind() == "super_interfaces" {
				self.emit_heritage_refs(child, parent, kinds::IMPLEMENTS, graph);
			}
		}
		self.emit_annotations_from(enum_node, parent, graph);

		let Some(body) = enum_node.child_by_field_name("body") else { return };
		let mut cursor = body.walk();
		for child in body.children(&mut cursor) {
			match child.kind() {
				"enum_constant" => {
					if let Some(name_node) = child.child_by_field_name("name") {
						let name = self.text_of(name_node);
						let m = extend_segment(parent, kinds::ENUM_CONSTANT, name.as_bytes());
						let _ = graph.add_def(
							m,
							kinds::ENUM_CONSTANT,
							parent,
							Some(node_position(child)),
						);
					}
				}
				"enum_body_declarations" => self.walk(child, parent, graph),
				_ => self.dispatch(child, parent, graph),
			}
		}
	}

	// --- callables -------------------------------------------------------

	fn handle_method(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		self.emit_callable(node, name.as_bytes(), kinds::METHOD, scope, graph);
	}

	fn handle_constructor(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
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
		let types = formal_parameter_types(node, self.source_bytes);
		let signature = types.join(",");
		let m = extend_callable_typed(scope, kind, name, &types);
		let attrs = DefAttrs {
			visibility: modifier_visibility(node),
			signature: signature.as_bytes(),
			..DefAttrs::default()
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kind,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		self.emit_annotations_from(node, &m, graph);

		// return type → uses_type
		if let Some(rt) = node.child_by_field_name("type") {
			self.emit_uses_type(rt, &m, graph);
		}

		self.push_local_scope();
		// parameters: record locals + emit param defs in deep mode
		if let Some(params) = node.child_by_field_name("parameters") {
			self.handle_parameters(params, &m, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
		self.pop_local_scope();
	}

	fn handle_parameters(
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
			let Some(name_node) = child.child_by_field_name("name") else { continue };
			let name = self.text_of(name_node);
			self.record_local(name.as_bytes());
			if self.deep {
				let m = extend_segment(callable, kinds::PARAM, name.as_bytes());
				let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(child)));
			}
			if let Some(t) = child.child_by_field_name("type") {
				self.emit_uses_type(t, callable, graph);
			}
		}
	}

	// --- fields / locals -------------------------------------------------

	fn handle_field(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let visibility = modifier_visibility(node);
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, parent, graph);
		}
		self.emit_annotations_from(node, parent, graph);

		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else { continue };
			let name = self.text_of(name_node);
			let m = extend_segment(parent, kinds::FIELD, name.as_bytes());
			let attrs = DefAttrs { visibility, ..DefAttrs::default() };
			let _ = graph.add_def_attrs(
				m.clone(),
				kinds::FIELD,
				parent,
				Some(node_position(child)),
				&attrs,
			);
			if let Some(value) = child.child_by_field_name("value") {
				self.dispatch(value, &m, graph);
			}
		}
	}

	fn handle_local_variable(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let inside_callable = is_callable_scope(scope, &self.module);
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else { continue };
			let name = self.text_of(name_node);
			if inside_callable {
				self.record_local(name.as_bytes());
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, name.as_bytes());
					let _ = graph.add_def(
						m,
						kinds::LOCAL,
						scope,
						Some(node_position(child)),
					);
				}
			}
			if let Some(value) = child.child_by_field_name("value") {
				self.dispatch(value, scope, graph);
			}
		}
	}

	// --- deep: catch / for-each / lambda --------------------------------

	fn handle_catch_param(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		// `catch (IOException e)` — `e` is a local in the enclosing
		// callable. The type child also needs a uses_type ref.
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let Some(name_node) = node.child_by_field_name("name") else { return };
		let name = self.text_of(name_node);
		if name.is_empty() {
			return;
		}
		if is_callable_scope(scope, &self.module) {
			self.record_local(name.as_bytes());
			if self.deep {
				let m = extend_segment(scope, kinds::PARAM, name.as_bytes());
				let _ = graph.add_def(m, kinds::PARAM, scope, Some(node_position(node)));
			}
		}
	}

	fn handle_enhanced_for(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		// `for (T x : iter) { ... }` — `x` is a local. Type goes through
		// uses_type; the iter expression and body still need normal walk.
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		if let Some(name_node) = node.child_by_field_name("name") {
			let name = self.text_of(name_node);
			if !name.is_empty() && is_callable_scope(scope, &self.module) {
				self.record_local(name.as_bytes());
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, name.as_bytes());
					let _ = graph.add_def(
						m,
						kinds::LOCAL,
						scope,
						Some(node_position(name_node)),
					);
				}
			}
		}
		if let Some(value) = node.child_by_field_name("value") {
			self.dispatch(value, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, scope, graph);
		}
	}

	fn handle_lambda(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		// `(a, b) -> a + b` / `a -> a` / `(int a) -> a`. Lambdas
		// introduce a new local frame so the body's references resolve
		// correctly. The lambda itself is not emitted as a def in the
		// MVP — its parameters are recorded as locals when the
		// surrounding scope is a callable.
		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			match params.kind() {
				"identifier" => {
					let name = self.text_of(params);
					if !name.is_empty() {
						self.record_local(name.as_bytes());
						if self.deep && is_callable_scope(scope, &self.module) {
							let m = extend_segment(scope, kinds::PARAM, name.as_bytes());
							let _ = graph.add_def(
								m,
								kinds::PARAM,
								scope,
								Some(node_position(params)),
							);
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
						let name = self.text_of(nn);
						if name.is_empty() {
							continue;
						}
						self.record_local(name.as_bytes());
						if self.deep && is_callable_scope(scope, &self.module) {
							let m = extend_segment(scope, kinds::PARAM, name.as_bytes());
							let _ = graph.add_def(
								m,
								kinds::PARAM,
								scope,
								Some(node_position(nn)),
							);
						}
					}
				}
				_ => {}
			}
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, scope, graph);
		}
		self.pop_local_scope();
	}

	// --- annotations -----------------------------------------------------

	fn emit_annotations_from(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() == "modifiers" {
				let mut mc = child.walk();
				for m in child.children(&mut mc) {
					if matches!(m.kind(), "marker_annotation" | "annotation") {
						self.handle_annotation(m, parent, graph);
					}
				}
			}
		}
	}

	// --- helpers ---------------------------------------------------------

	pub(super) fn field_text(&self, node: Node<'_>, field: &str) -> Option<&'src str> {
		node.child_by_field_name(field)?
			.utf8_text(self.source_bytes)
			.ok()
	}

	pub(super) fn text_of(&self, node: Node<'_>) -> &'src str {
		node.utf8_text(self.source_bytes).unwrap_or("")
	}
}

/// Pre-pass: walk every type declaration (top-level + nested) under
/// `node` and record `short-name → moniker` so refs to those names
/// can be tagged `resolved` with a real target.
pub(super) fn collect_type_table<'src>(
	node: Node<'_>,
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
		let Some(name_node) = child.child_by_field_name("name") else { continue };
		let Ok(name) = name_node.utf8_text(source) else { continue };
		let m = extend_segment(parent, kind, name.as_bytes());
		// First-write-wins: a top-level type shouldn't be shadowed by a
		// later nested namesake.
		out.entry(name.as_bytes()).or_insert_with(|| m.clone());
		if let Some(body) = child.child_by_field_name("body") {
			collect_type_table(body, source, &m, out);
		}
	}
}

/// Parameter type list as it appears in source (short type names,
/// generics and array suffixes preserved). Drives both the typed
/// callable moniker (`method:bar(int,String)`) and `DefRecord.signature`
/// for projection-side filtering.
pub(super) fn formal_parameter_types<'src>(
	callable: Node<'_>,
	source: &'src [u8],
) -> Vec<&'src str> {
	let Some(params) = callable.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let mut cursor = params.walk();
	for c in params.named_children(&mut cursor) {
		if !matches!(c.kind(), "formal_parameter" | "spread_parameter") {
			continue;
		}
		let Some(t) = c.child_by_field_name("type") else { continue };
		let Ok(text) = t.utf8_text(source) else { continue };
		out.push(text.trim());
	}
	out
}
