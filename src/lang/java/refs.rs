//! Refs extraction: imports, calls, method_call (with receiver hint),
//! instantiates, extends, implements, annotates, uses_type, reads.

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{callable_segment_name, extend_segment, node_position};
use super::kinds;
use super::walker::Walker;

impl<'src> Walker<'src> {
	// --- imports ---------------------------------------------------------

	pub(super) fn handle_import(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
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
		let dotted = self.text_of(path_node);
		if dotted.is_empty() {
			return;
		}

		let pieces: Vec<&str> = dotted.split('.').collect();
		let confidence = external_or_imported(&pieces);

		if wildcard {
			let target = external_package_target(self.module.as_view().project(), &pieces);
			let attrs = RefAttrs { confidence, ..RefAttrs::default() };
			let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
			return;
		}

		// Named import: last piece is the symbol; record it for confidence.
		if let Some(last) = pieces.last().copied() {
			self.imports
				.borrow_mut()
				.insert(last.as_bytes(), confidence);
		}
		let target = external_symbol_target(self.module.as_view().project(), &pieces);
		let attrs = RefAttrs { confidence, ..RefAttrs::default() };
		let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_SYMBOL, Some(pos), &attrs);
	}

	// --- calls / new -----------------------------------------------------

	pub(super) fn handle_method_invocation(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let pos = node_position(node);
		let arity = argument_count(node);

		let object = node.child_by_field_name("object");
		let Some(name_node) = node.child_by_field_name("name") else {
			self.walk(node, scope, graph);
			return;
		};
		let name = self.text_of(name_node);
		if name.is_empty() {
			self.walk(node, scope, graph);
			return;
		}

		if let Some(obj) = object {
			let target = self.method_call_target(name, arity);
			let attrs = RefAttrs {
				receiver_hint: receiver_hint(obj),
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
			self.dispatch(obj, scope, graph);
		} else {
			let target = self.calls_target(name, arity);
			let confidence = self
				.import_confidence_for(name.as_bytes())
				.unwrap_or_else(|| self.name_confidence(name.as_bytes()));
			let attrs = RefAttrs { confidence, ..RefAttrs::default() };
			let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
		}

		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk(args, scope, graph);
		}
	}

	pub(super) fn handle_object_creation(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let pos = node_position(node);
		if let Some(t) = node.child_by_field_name("type") {
			let name = match t.kind() {
				"type_identifier" => self.text_of(t),
				"scoped_type_identifier" => last_identifier(t, self.source_bytes),
				"generic_type" => generic_type_short(t, self.source_bytes),
				_ => "",
			};
			if !name.is_empty() {
				let (target, confidence) =
					self.resolve_type_target(name.as_bytes(), kinds::CLASS);
				let attrs = RefAttrs { confidence, ..RefAttrs::default() };
				let _ = graph.add_ref_attrs(
					scope,
					target,
					kinds::INSTANTIATES,
					Some(pos),
					&attrs,
				);
			}
		}
		self.walk(node, scope, graph);
	}

	// --- heritage / annotations -----------------------------------------

	pub(super) fn emit_heritage_refs(
		&self,
		clause: Node<'_>,
		scope: &Moniker,
		edge: &[u8],
		graph: &mut CodeGraph,
	) {
		let mut cursor = clause.walk();
		for child in clause.named_children(&mut cursor) {
			let name = match child.kind() {
				"type_identifier" => self.text_of(child).to_string(),
				"scoped_type_identifier" => last_identifier(child, self.source_bytes).to_string(),
				"generic_type" => generic_type_short(child, self.source_bytes).to_string(),
				"type_list" => {
					self.emit_heritage_refs(child, scope, edge, graph);
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
			let attrs = RefAttrs { confidence, ..RefAttrs::default() };
			let _ = graph.add_ref_attrs(scope, target, edge, Some(node_position(child)), &attrs);
		}
	}

	pub(super) fn handle_annotation(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let pos = node_position(node);
		let Some(name_node) = node.child_by_field_name("name") else { return };
		let name = match name_node.kind() {
			"identifier" => self.text_of(name_node).to_string(),
			"scoped_identifier" => last_identifier(name_node, self.source_bytes).to_string(),
			_ => return,
		};
		if name.is_empty() {
			return;
		}
		let (target, confidence) =
			self.resolve_type_target(name.as_bytes(), kinds::ANNOTATION_TYPE);
		let attrs = RefAttrs { confidence, ..RefAttrs::default() };
		let _ = graph.add_ref_attrs(scope, target, kinds::ANNOTATES, Some(pos), &attrs);
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk(args, scope, graph);
		}
	}

	// --- type uses -------------------------------------------------------

	pub(super) fn emit_uses_type(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let name = match node.kind() {
			"type_identifier" => self.text_of(node).to_string(),
			"scoped_type_identifier" => last_identifier(node, self.source_bytes).to_string(),
			"generic_type" => {
				let head = generic_type_short(node, self.source_bytes).to_string();
				if let Some(args) = node.child_by_field_name("type_arguments") {
					self.walk(args, scope, graph);
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
				self.walk(node, scope, graph);
				return;
			}
		};
		if name.is_empty() {
			return;
		}
		let (target, confidence) = self.resolve_type_target(name.as_bytes(), kinds::CLASS);
		let attrs = RefAttrs { confidence, ..RefAttrs::default() };
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::USES_TYPE,
			Some(node_position(node)),
			&attrs,
		);
	}

	// --- reads -----------------------------------------------------------

	pub(super) fn handle_identifier(
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
		let confidence = self
			.import_confidence_for(name.as_bytes())
			.unwrap_or_else(|| self.name_confidence(name.as_bytes()));
		let attrs = RefAttrs { confidence, ..RefAttrs::default() };
		let _ = graph.add_ref_attrs(scope, target, kinds::READS, Some(node_position(node)), &attrs);
	}

	// --- target builders -------------------------------------------------

	fn calls_target(&self, name: &str, arity: u16) -> Moniker {
		let segment_name = callable_segment_name(name.as_bytes(), arity);
		let mut b = MonikerBuilder::from_view(self.module.as_view());
		b.segment(kinds::METHOD, &segment_name);
		b.build()
	}

	fn method_call_target(&self, name: &str, arity: u16) -> Moniker {
		self.calls_target(name, arity)
	}

	fn read_target(&self, name: &str) -> Moniker {
		extend_segment(&self.module, kinds::FIELD, name.as_bytes())
	}
}

/// Count argument-list children of a `method_invocation` /
/// `object_creation_expression`'s `arguments` field.
fn argument_count(call: Node<'_>) -> u16 {
	let Some(args) = call.child_by_field_name("arguments") else { return 0 };
	let mut cursor = args.walk();
	let mut count: u16 = 0;
	for c in args.named_children(&mut cursor) {
		count = count.saturating_add(1);
		let _ = c;
	}
	count
}

/// Receiver shape for a method_invocation's `object`. Mirrors the TS
/// receiver_hint vocabulary: this/super/identifier/member/call.
fn receiver_hint(obj: Node<'_>) -> &'static [u8] {
	match obj.kind() {
		"this" => b"this",
		"super" => b"super",
		"identifier" => b"identifier",
		"method_invocation" => b"call",
		"field_access" => b"member",
		"scoped_identifier" => b"member",
		_ => b"",
	}
}

/// Last identifier text under a `scoped_identifier` /
/// `scoped_type_identifier` (`com.acme.Foo` → `Foo`).
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

/// Short type name from a `generic_type` (`List<String>` → `List`).
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

/// Build an external-package target for an import path. JDK packages
/// (`java.*`, `javax.*`) and any non-relative dotted path land under
/// `external_pkg:<head>` with the rest as `path:` segments.
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

/// Build a target for `import com.acme.Foo;` — the trailing `Foo` is
/// the imported symbol; the leading `com.acme` is the external pkg.
fn external_symbol_target(project: &[u8], pieces: &[&str]) -> Moniker {
	external_package_target(project, pieces)
}

/// Heuristic: anything starting with `java.` / `javax.` / `kotlin.` /
/// `sun.` / `com.sun.` is JDK external. Everything else without a `.`
/// is project-local-but-FQN-unknown so we tag `imported` (the
/// consumer can refine with a package preset later).
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
