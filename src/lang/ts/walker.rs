//! AST traversal for tree-sitter-typescript: dispatches each node to
//! its def emitter or to the refs module. Scope = innermost enclosing
//! def; it doubles as `parent` for new defs and as `source` for refs.

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, DefAttrs};
use crate::core::moniker::Moniker;

use super::canonicalize::{
	anonymous_callback_name, callable_arity, extend_method, extend_segment, node_position,
};
use super::kinds;

pub(super) struct Walker<'src> {
	pub(super) source_bytes: &'src [u8],
	pub(super) module: Moniker,
	pub(super) deep: bool,
	pub(super) presets: &'src super::Presets,
	/// Byte ranges of top-level `export_statement` nodes. Lets module-
	/// scope def emitters answer "am I exported?" without looking at
	/// the AST parent.
	pub(super) export_ranges: Vec<(u32, u32)>,
}

/// Pre-pass: collect every top-level `export_statement` node's byte range.
pub(super) fn collect_export_ranges(root: Node<'_>) -> Vec<(u32, u32)> {
	let mut out = Vec::new();
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() == "export_statement" {
			out.push((child.start_byte() as u32, child.end_byte() as u32));
		}
	}
	out
}

impl<'src> Walker<'src> {
	/// True when `node` lives inside a top-level `export_statement`. Used
	/// to derive `public` vs `module` visibility for module-scope decls.
	pub(super) fn is_exported(&self, node: Node<'_>) -> bool {
		let s = node.start_byte() as u32;
		self.export_ranges.iter().any(|(a, b)| *a <= s && s < *b)
	}

	/// Module-scope def visibility: `public` if wrapped by an export,
	/// `module` otherwise. For class/interface members, callers use
	/// `member_visibility(node)` instead.
	pub(super) fn module_visibility(&self, node: Node<'_>) -> &'static [u8] {
		if self.is_exported(node) {
			kinds::VIS_PUBLIC
		} else {
			kinds::VIS_MODULE
		}
	}

	/// Top-level entry. Walks every child of `node` and dispatches it.
	/// `scope` is the moniker that defs nest under and refs source on.
	pub(super) fn walk(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			self.dispatch(child, scope, graph);
		}
	}

	pub(super) fn dispatch(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		match node.kind() {
			"comment" => self.handle_comment(node, scope, graph),
			"import_statement" => self.handle_import(node, scope, graph),
			"export_statement" => self.handle_export(node, scope, graph),
			"class_declaration" | "abstract_class_declaration" => {
				self.handle_class(node, scope, graph)
			}
			"interface_declaration" => self.handle_interface(node, scope, graph),
			"enum_declaration" => self.handle_enum(node, scope, graph),
			"type_alias_declaration" => self.handle_type_alias(node, scope, graph),
			"function_declaration" | "generator_function_declaration" => {
				self.handle_function_decl(node, scope, graph)
			}
			"lexical_declaration" | "variable_declaration" => {
				self.handle_lexical(node, scope, graph)
			}
			"call_expression" => self.handle_call(node, scope, graph),
			"new_expression" => self.handle_new(node, scope, graph),
			"decorator" => self.handle_decorator(node, scope, graph),
			"type_annotation" | "type_arguments" | "union_type" | "intersection_type"
			| "lookup_type" | "index_type_query" | "type_query" | "generic_type"
			| "nested_type_identifier" => {
				self.emit_uses_type_recursive(node, scope, graph);
			}
			"return_statement" | "spread_element" | "parenthesized_expression"
			| "template_substitution" | "arguments" | "array" => {
				self.emit_reads_in_children(node, scope, graph);
			}
			"binary_expression" | "assignment_expression" => {
				self.handle_binary_like(node, scope, graph);
			}
			"unary_expression" | "update_expression" => {
				self.handle_unary_like(node, scope, graph);
			}
			"ternary_expression" => self.handle_ternary(node, scope, graph),
			"member_expression" | "subscript_expression" => {
				self.handle_member_like(node, scope, graph);
			}
			"shorthand_property_identifier" => self.emit_read_at(node, scope, graph),
			"jsx_expression" => self.emit_reads_in_children(node, scope, graph),
			"jsx_opening_element" | "jsx_self_closing_element" => {
				self.handle_jsx_element(node, scope, graph)
			}
			"pair" => self.handle_pair(node, scope, graph),
			"arrow_function" | "function_expression" => {
				self.handle_inline_callable(node, scope, graph)
			}
			"catch_clause" => self.handle_catch_clause(node, scope, graph),
			"for_in_statement" | "for_of_statement" => {
				self.handle_for_in(node, scope, graph)
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

	// --- export_statement ------------------------------------------------

	fn handle_export(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if node.child_by_field_name("source").is_some() {
			self.handle_reexport(node, scope, graph);
			return;
		}
		// `export default function f() {…}` / `export default class C {…}`:
		// emit the named decl if present, else a `default` placeholder.
		let mut cursor = node.walk();
		let mut has_default = false;
		for c in node.children(&mut cursor) {
			if c.kind() == "default" {
				has_default = true;
				break;
			}
		}
		if has_default {
			let public = DefAttrs { visibility: kinds::VIS_PUBLIC };
			let mut cursor = node.walk();
			for c in node.children(&mut cursor) {
				match c.kind() {
					"function_expression" | "arrow_function" => {
						let m = extend_method(scope, kinds::FUNCTION, b"default", callable_arity(c));
						let _ = graph.add_def_attrs(
							m.clone(),
							kinds::FUNCTION,
							scope,
							Some(node_position(c)),
							&public,
						);
						if let Some(body) = c.child_by_field_name("body") {
							self.walk(body, &m, graph);
						}
						return;
					}
					"class" | "class_declaration" => {
						let m = extend_segment(scope, kinds::CLASS, b"default");
						let _ = graph.add_def_attrs(
							m.clone(),
							kinds::CLASS,
							scope,
							Some(node_position(c)),
							&public,
						);
						if let Some(body) = c.child_by_field_name("body") {
							self.walk_class_body(body, &m, graph);
						}
						return;
					}
					_ => {}
				}
			}
		}
		self.walk(node, scope, graph);
	}

	// --- class -----------------------------------------------------------

	fn handle_class(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_segment(scope, kinds::CLASS, name.as_bytes());
		let attrs = DefAttrs { visibility: self.module_visibility(node) };
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::CLASS,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"class_heritage" => self.handle_class_heritage(child, &m, graph),
				"decorator" => self.handle_decorator(child, &m, graph),
				"class_body" => self.walk_class_body(child, &m, graph),
				_ => {}
			}
		}
	}

	fn walk_class_body(&self, body: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = body.walk();
		for child in body.children(&mut cursor) {
			match child.kind() {
				"method_definition" | "method_signature" => {
					self.handle_method(child, parent, graph)
				}
				"public_field_definition" | "property_signature" => {
					self.handle_field(child, parent, graph)
				}
				"decorator" => self.handle_decorator(child, parent, graph),
				_ => {}
			}
		}
	}

	fn handle_method(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let arity = callable_arity(node);
		let is_ctor = name == "constructor";
		let kind: &[u8] = if is_ctor { kinds::CONSTRUCTOR } else { kinds::METHOD };
		let m = extend_method(parent, kind, name.as_bytes(), arity);
		let attrs = DefAttrs {
			visibility: class_member_visibility(node, self.source_bytes),
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kind,
			parent,
			Some(node_position(node)),
			&attrs,
		);

		// decorators on the method itself
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "decorator" {
				self.handle_decorator(c, &m, graph);
			}
		}

		// return type → uses_type
		if let Some(rt) = node.child_by_field_name("return_type") {
			self.emit_uses_type_recursive(rt, &m, graph);
		}

		// parameters
		if let Some(params) = node.child_by_field_name("parameters") {
			self.handle_parameters(params, &m, graph);
		}

		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
	}

	fn handle_field(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_segment(parent, kinds::FIELD, name.as_bytes());
		let attrs = DefAttrs {
			visibility: class_member_visibility(node, self.source_bytes),
		};
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::FIELD,
			parent,
			Some(node_position(node)),
			&attrs,
		);

		// decorators on field
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "decorator" {
				self.handle_decorator(c, &m, graph);
			}
		}

		if let Some(tp) = node.child_by_field_name("type") {
			self.emit_uses_type_recursive(tp, &m, graph);
		}
		// initializer expression: walk under the field as scope so any
		// `new Foo()` etc. attribute correctly.
		if let Some(value) = node.child_by_field_name("value") {
			self.dispatch(value, &m, graph);
		}
	}

	// --- interface / enum / type_alias -----------------------------------

	fn handle_interface(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_segment(scope, kinds::INTERFACE, name.as_bytes());
		let attrs = DefAttrs { visibility: self.module_visibility(node) };
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::INTERFACE,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"extends_type_clause" | "extends_clause" => {
					self.emit_heritage_refs(child, &m, kinds::EXTENDS, graph);
				}
				"interface_body" | "object_type" => {
					let mut bc = child.walk();
					for member in child.children(&mut bc) {
						match member.kind() {
							"property_signature" => self.handle_field(member, &m, graph),
							"method_signature" | "method_definition" => {
								self.handle_method(member, &m, graph)
							}
							_ => {}
						}
					}
				}
				_ => {}
			}
		}
	}

	fn handle_enum(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_segment(scope, kinds::ENUM, name.as_bytes());
		let attrs = DefAttrs { visibility: self.module_visibility(node) };
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::ENUM,
			scope,
			Some(node_position(node)),
			&attrs,
		);

		if let Some(body) = node.child_by_field_name("body") {
			let mut cursor = body.walk();
			for member in body.named_children(&mut cursor) {
				if member.kind() == "enum_assignment" || member.kind() == "property_identifier" {
					let name_node = if member.kind() == "enum_assignment" {
						member.child_by_field_name("name").unwrap_or(member)
					} else {
						member
					};
					if let Ok(member_name) = name_node.utf8_text(self.source_bytes) {
						let mm = extend_segment(&m, kinds::ENUM_CONSTANT, member_name.as_bytes());
						let _ = graph.add_def(
							mm,
							kinds::ENUM_CONSTANT,
							&m,
							Some(node_position(member)),
						);
					}
				}
			}
		}
	}

	fn handle_type_alias(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let m = extend_segment(scope, kinds::TYPE_ALIAS, name.as_bytes());
		let attrs = DefAttrs { visibility: self.module_visibility(node) };
		let _ = graph.add_def_attrs(
			m.clone(),
			kinds::TYPE_ALIAS,
			scope,
			Some(node_position(node)),
			&attrs,
		);
		if let Some(value) = node.child_by_field_name("value") {
			self.emit_uses_type_recursive(value, &m, graph);
		}
	}

	// --- function / lexical-declaration ----------------------------------

	fn handle_function_decl(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(name) = self.field_text(node, "name") else { return };
		let vis = self.module_visibility(node);
		self.emit_callable(node, node, name.as_bytes(), kinds::FUNCTION, scope, graph, vis);
	}

	fn handle_lexical(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let inside_callable = is_callable_scope(scope, &self.module);
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			self.handle_variable_declarator(child, scope, inside_callable, graph);
		}
	}

	fn handle_variable_declarator(
		&self,
		decl: Node<'_>,
		scope: &Moniker,
		inside_callable: bool,
		graph: &mut CodeGraph,
	) {
		let Some(name_node) = decl.child_by_field_name("name") else { return };
		let value = decl.child_by_field_name("value");
		let type_annot = decl.child_by_field_name("type");

		// Collect identifier-or-pattern names.
		let names = collect_binding_names(name_node, self.source_bytes);

		let module_vis = self.module_visibility(decl);
		for name in &names {
			let (kind, emit) = if inside_callable {
				(kinds::LOCAL, self.deep)
			} else if let Some(v) = value.filter(|v| {
				v.kind() == "arrow_function" || v.kind() == "function_expression"
			}) {
				self.emit_callable(
					v,
					decl,
					name.as_bytes(),
					kinds::FUNCTION,
					scope,
					graph,
					module_vis,
				);
				continue;
			} else {
				(kinds::CONST, true)
			};
			if emit {
				let m = extend_segment(scope, kind, name.as_bytes());
				let attrs = DefAttrs {
					visibility: if inside_callable { kinds::VIS_PRIVATE } else { module_vis },
				};
				let _ = graph.add_def_attrs(m, kind, scope, Some(node_position(decl)), &attrs);
			}
		}

		if let Some(tp) = type_annot {
			self.emit_uses_type_recursive(tp, scope, graph);
		}
		if let Some(v) = value {
			self.dispatch(v, scope, graph);
		}
	}

	// --- parameters ------------------------------------------------------

	fn handle_parameters(
		&self,
		params: Node<'_>,
		callable: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"required_parameter" | "optional_parameter" => {
					let pat = child.child_by_field_name("pattern");
					let tp = child.child_by_field_name("type");
					if let Some(p) = pat {
						self.emit_param_leaf(p, callable, graph);
					}
					if let Some(t) = tp {
						self.emit_uses_type_recursive(t, callable, graph);
					}
					// decorators on parameter (TS parameter properties)
					let mut cc = child.walk();
					for c in child.children(&mut cc) {
						if c.kind() == "decorator" {
							self.handle_decorator(c, callable, graph);
						}
					}
				}
				"rest_pattern" => {
					self.emit_param_leaf(child, callable, graph);
				}
				_ => {}
			}
		}
	}

	fn emit_param_leaf(&self, pat: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
		if !self.deep {
			return;
		}
		for name in collect_binding_names(pat, self.source_bytes) {
			let m = extend_segment(callable, kinds::PARAM, name.as_bytes());
			let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(pat)));
		}
	}

	// --- inline callables, catch, for-in ---------------------------------

	fn handle_inline_callable(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if self.deep && is_callable_scope(scope, &self.module) {
			let name = anonymous_callback_name(node);
			self.emit_callable(
				node,
				node,
				&name,
				kinds::FUNCTION,
				scope,
				graph,
				kinds::VIS_PRIVATE,
			);
			return;
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			self.walk(params, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, scope, graph);
		}
	}

	fn handle_catch_clause(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if self.deep && is_callable_scope(scope, &self.module) {
			if let Some(p) = node.child_by_field_name("parameter") {
				self.emit_param_leaf(p, scope, graph);
			}
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, scope, graph);
		}
	}

	fn handle_for_in(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if self.deep && is_callable_scope(scope, &self.module) {
			let mut cursor = node.walk();
			for c in node.named_children(&mut cursor) {
				if c.kind() == "identifier" {
					let m = extend_segment(scope, kinds::LOCAL, self.text_of(c).as_bytes());
					let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(c)));
					break;
				}
			}
		}
		self.walk(node, scope, graph);
	}

	// --- pair (object literal) -------------------------------------------

	fn handle_pair(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		if self.deep && is_callable_scope(scope, &self.module) {
			let key = node.child_by_field_name("key");
			let value = node.child_by_field_name("value");
			if let (Some(k), Some(v)) = (key, value) {
				if k.kind() == "property_identifier"
					&& (v.kind() == "arrow_function" || v.kind() == "function_expression")
				{
					let name = self.text_of(k);
					self.emit_callable(
						v,
						node,
						name.as_bytes(),
						kinds::FUNCTION,
						scope,
						graph,
						kinds::VIS_PUBLIC,
					);
					return;
				}
			}
		}
		self.walk(node, scope, graph);
	}

	/// Emit a callable def + its parameters + recurse into its body.
	/// Shared between `function_declaration`, arrow/function bound to a
	/// const, anonymous callbacks, and `{ key: () => … }` shorthand.
	fn emit_callable(
		&self,
		callable_node: Node<'_>,
		anchor_node: Node<'_>,
		name: &[u8],
		kind: &[u8],
		parent: &Moniker,
		graph: &mut CodeGraph,
		visibility: &[u8],
	) -> Moniker {
		let arity = callable_arity(callable_node);
		let m = extend_method(parent, kind, name, arity);
		let attrs = DefAttrs { visibility };
		let _ = graph.add_def_attrs(
			m.clone(),
			kind,
			parent,
			Some(node_position(anchor_node)),
			&attrs,
		);
		if let Some(rt) = callable_node.child_by_field_name("return_type") {
			self.emit_uses_type_recursive(rt, &m, graph);
		}
		if let Some(params) = callable_node.child_by_field_name("parameters") {
			self.handle_parameters(params, &m, graph);
		}
		if let Some(p) = callable_node.child_by_field_name("parameter") {
			self.emit_param_leaf(p, &m, graph);
		}
		if let Some(body) = callable_node.child_by_field_name("body") {
			self.walk(body, &m, graph);
		}
		m
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

/// Collect leaf identifier names from a binding pattern (`identifier`,
/// `object_pattern`, `array_pattern`, `rest_pattern`). Empty for `_`-style
/// untargeted holes.
fn collect_binding_names(pat: Node<'_>, source: &[u8]) -> Vec<String> {
	fn rec(node: Node<'_>, source: &[u8], out: &mut Vec<String>) {
		match node.kind() {
			"identifier" | "shorthand_property_identifier_pattern" => {
				if let Ok(s) = node.utf8_text(source) {
					out.push(s.to_string());
				}
			}
			"object_pattern" | "array_pattern" | "pair_pattern" | "rest_pattern"
			| "assignment_pattern" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					rec(c, source, out);
				}
			}
			_ => {}
		}
	}
	let mut out = Vec::new();
	rec(pat, source, &mut out);
	out
}

/// True when `scope` is the moniker of a callable def — i.e. we're inside
/// a function/method/constructor body.
fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else { return false };
	last.kind == kinds::FUNCTION || last.kind == kinds::METHOD || last.kind == kinds::CONSTRUCTOR
}

/// Class member visibility: explicit modifier when present, `public` by
/// default (TS class members default to public).
pub(super) fn class_member_visibility(node: Node<'_>, source: &[u8]) -> &'static [u8] {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "accessibility_modifier" {
			return match c.utf8_text(source).unwrap_or("") {
				"private" => kinds::VIS_PRIVATE,
				"protected" => kinds::VIS_PROTECTED,
				"public" => kinds::VIS_PUBLIC,
				_ => kinds::VIS_PUBLIC,
			};
		}
	}
	kinds::VIS_PUBLIC
}

/// ESAC TS recognises section comments shaped like
/// `// ===== Title =====` or `/* === Title === */`. Returns the trimmed
/// title when the comment matches.
fn section_title<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
	let raw = node.utf8_text(source).ok()?;
	let body = raw
		.strip_prefix("//")
		.or_else(|| raw.strip_prefix("/*").and_then(|s| s.strip_suffix("*/")))
		.unwrap_or(raw);
	let body = body.trim();
	let stripped = body.trim_matches(|c: char| c == '=' || c == '-' || c.is_whitespace());
	if stripped.is_empty() {
		return None;
	}
	let starts = body.starts_with("==") || body.starts_with("--");
	let ends = body.ends_with("==") || body.ends_with("--");
	if starts && ends {
		Some(stripped)
	} else {
		None
	}
}
