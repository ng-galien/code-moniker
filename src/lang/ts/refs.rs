//! Refs extraction for TypeScript: imports, reexports, calls,
//! method_call, instantiates, extends/implements, decorators,
//! uses_type, reads, di_register.
//!
//! Targets are name-keyed monikers under the importing module. Cross-
//! module resolution is the consumer's job — ESAC's projection layer
//! intersects targets with def monikers across the corpus.

use tree_sitter::Node;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{
	append_path_segments, callable_segment_name, extend_method, extend_segment,
	external_pkg_builder, node_position, strip_known_extension,
};
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
		let Some(src_node) = node.child_by_field_name("source") else { return };
		let raw_spec = unquote_string_literal(src_node, self.source_bytes);
		if raw_spec.is_empty() {
			return;
		}
		let pos = node_position(node);

		let mut clause: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "import_clause" {
				clause = Some(c);
				break;
			}
		}

		let Some(clause) = clause else {
			// `import 'side-effect'` — emit a module-level import.
			let target = self.import_module_target(&raw_spec);
			let _ = graph.add_ref(scope, target, kinds::IMPORTS_MODULE, Some(pos));
			return;
		};

		let mut cursor = clause.walk();
		for c in clause.children(&mut cursor) {
			match c.kind() {
				"identifier" => {
					// `import Default from './foo'`
					let local_name = self.text_of(c);
					let target = self.import_symbol_target(&raw_spec, "default");
					let _ = graph.add_ref(scope, target, kinds::IMPORTS_SYMBOL, Some(pos));
					let _ = local_name; // alias goes nowhere until RefRecord gains metadata
				}
				"namespace_import" => {
					let target = self.import_module_target(&raw_spec);
					let _ = graph.add_ref(scope, target, kinds::IMPORTS_MODULE, Some(pos));
				}
				"named_imports" => {
					let mut nc = c.walk();
					for spec in c.children(&mut nc) {
						if spec.kind() != "import_specifier" {
							continue;
						}
						let name = spec
							.child_by_field_name("name")
							.map(|n| self.text_of(n))
							.unwrap_or("");
						if name.is_empty() {
							continue;
						}
						let target = self.import_symbol_target(&raw_spec, name);
						let _ = graph.add_ref(scope, target, kinds::IMPORTS_SYMBOL, Some(pos));
					}
				}
				_ => {}
			}
		}
	}

	pub(super) fn handle_reexport(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(src_node) = node.child_by_field_name("source") else { return };
		let raw_spec = unquote_string_literal(src_node, self.source_bytes);
		if raw_spec.is_empty() {
			return;
		}
		let pos = node_position(node);

		let mut has_star = false;
		let mut export_clause: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"*" => has_star = true,
				"export_clause" => export_clause = Some(c),
				_ => {}
			}
		}

		if has_star {
			let target = self.import_module_target(&raw_spec);
			let _ = graph.add_ref(scope, target, kinds::REEXPORTS, Some(pos));
			return;
		}

		let Some(clause) = export_clause else { return };
		let mut nc = clause.walk();
		for spec in clause.children(&mut nc) {
			if spec.kind() != "export_specifier" {
				continue;
			}
			let name = spec
				.child_by_field_name("name")
				.map(|n| self.text_of(n))
				.unwrap_or("");
			if name.is_empty() {
				continue;
			}
			let target = self.import_symbol_target(&raw_spec, name);
			let _ = graph.add_ref(scope, target, kinds::REEXPORTS, Some(pos));
		}
	}

	/// Build a target for `imports_module` (whole module, namespace import,
	/// star reexport) and `import 'side-effect'`. Bare specifier ⇒ external,
	/// relative ⇒ resolved against the importer's directory.
	fn import_module_target(&self, raw_path: &str) -> Moniker {
		self.import_target(raw_path, None)
	}

	fn import_symbol_target(&self, raw_path: &str, name: &str) -> Moniker {
		self.import_target(raw_path, Some(name))
	}

	/// Single-pass builder. `symbol` appended as `path:<name>` if Some.
	fn import_target(&self, raw_path: &str, symbol: Option<&str>) -> Moniker {
		let mut b = if is_relative_specifier(raw_path) {
			self.relative_module_builder(raw_path)
		} else {
			external_pkg_builder(self.module.as_view().project(), raw_path)
		};
		if let Some(sym) = symbol {
			b.segment(kinds::PATH, sym.as_bytes());
		}
		b.build()
	}

	fn relative_module_builder(&self, raw_path: &str) -> MonikerBuilder {
		let importer_view = self.module.as_view();
		let mut b = MonikerBuilder::from_view(importer_view);
		let mut depth = (importer_view.segment_count() as usize).saturating_sub(1);
		b.truncate(depth);

		// Normalise the dot-only shorthands to their slash form so the
		// loop below handles them uniformly.
		let mut remainder = match raw_path {
			"." => "./",
			".." => "../",
			other => other,
		};
		while let Some(rest) = remainder.strip_prefix("./") {
			remainder = rest;
		}
		while let Some(rest) = remainder.strip_prefix("../") {
			depth = depth.saturating_sub(1);
			b.truncate(depth);
			remainder = rest;
		}
		let remainder = strip_known_extension(remainder);
		append_path_segments(&mut b, remainder, kinds::PATH);
		b
	}

	// --- calls / new -----------------------------------------------------

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
				let _ = graph.add_ref(scope, target, kinds::CALLS, Some(pos));
				self.maybe_emit_di_register(node, fn_node, scope, graph, pos);
			}
			"member_expression" => {
				if let Some(prop) = fn_node.child_by_field_name("property") {
					let name = self.text_of(prop);
					if !name.is_empty() {
						let target = self.method_call_target(name, arity);
						let recv = receiver_hint(fn_node);
						let _ = graph.add_ref_with_meta(
							scope,
							target,
							kinds::METHOD_CALL,
							Some(pos),
							recv,
						);
					}
				}
				if let Some(obj) = fn_node.child_by_field_name("object") {
					self.dispatch(obj, scope, graph);
				}
			}
			_ => {}
		}

		// arguments may contain nested calls/reads/etc.
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
					let _ = graph.add_ref(scope, target, kinds::INSTANTIATES, Some(pos));
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
		// Heuristic only fires when the callee identifier is in the
		// caller-supplied preset list (e.g. ['register','bind','provide']).
		// Without a preset, every `it(name)` and `expect(value)` would
		// otherwise be tagged as DI registration.
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
		let _ = graph.add_ref(scope, target, kinds::DI_REGISTER, Some(pos));
	}

	// --- class / interface heritage --------------------------------------

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
			let _ = graph.add_ref(scope, target, edge, Some(pos));
		}
	}

	// --- decorators ------------------------------------------------------

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
						let _ = graph.add_ref(scope, target, kinds::ANNOTATES, Some(pos));
					}
				}
				"call_expression" => {
					if let Some(fn_node) = c.child_by_field_name("function") {
						if fn_node.kind() == "identifier" {
							let name = self.text_of(fn_node);
							let arity = call_argument_count(c);
							let target = self.calls_target(name, arity);
							let _ = graph.add_ref(scope, target, kinds::ANNOTATES, Some(pos));
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

	// --- uses_type -------------------------------------------------------

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
				let _ = graph.add_ref(scope, target, kinds::USES_TYPE, Some(node_position(node)));
			}
			"nested_type_identifier" => {
				if let Some(name) = nested_type_short(node, self.source_bytes) {
					let target = self.heritage_target(kinds::CLASS, &name);
					let _ = graph.add_ref(
						scope,
						target,
						kinds::USES_TYPE,
						Some(node_position(node)),
					);
				}
			}
			"generic_type" => {
				if let Some(name) = generic_short(node, self.source_bytes) {
					let target = self.heritage_target(kinds::CLASS, &name);
					let _ = graph.add_ref(
						scope,
						target,
						kinds::USES_TYPE,
						Some(node_position(node)),
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

	// --- reads -----------------------------------------------------------

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
		let _ = graph.add_ref(scope, target, kinds::READS, Some(node_position(node)));
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

	// --- target builders -------------------------------------------------

	fn calls_target(&self, name: &str, arity: u16) -> Moniker {
		let segment_name = callable_segment_name(name.as_bytes(), arity);
		let mut b = MonikerBuilder::from_view(self.module.as_view());
		b.segment(kinds::FUNCTION, &segment_name);
		b.build()
	}

	fn method_call_target(&self, name: &str, arity: u16) -> Moniker {
		let segment_name = callable_segment_name(name.as_bytes(), arity);
		let mut b = MonikerBuilder::from_view(self.module.as_view());
		b.segment(kinds::METHOD, &segment_name);
		b.build()
	}

	fn instantiates_target(&self, name: &str) -> Moniker {
		extend_segment(&self.module, kinds::CLASS, name.as_bytes())
	}

	fn heritage_target(&self, kind: &[u8], name: &str) -> Moniker {
		extend_segment(&self.module, kind, name.as_bytes())
	}

	fn read_target(&self, name: &str) -> Moniker {
		extend_method(&self.module, kinds::FUNCTION, name.as_bytes(), 0)
	}
}

/// True for `./x`, `../x`, and the dot-only shorthands `.` and `..`.
fn is_relative_specifier(spec: &str) -> bool {
	spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../")
}

/// Classify the `object` side of a `member_expression` callee. Empty
/// slice when the receiver shape isn't one we recognise — caller stores
/// it in `RefRecord.meta` and an empty value means "no hint".
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

fn unquote_string_literal<'src>(node: Node<'_>, source: &'src [u8]) -> &'src str {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "string_fragment" {
			if let Ok(s) = c.utf8_text(source) {
				return s;
			}
		}
	}
	node.utf8_text(source)
		.unwrap_or("")
		.trim_matches(|c| c == '"' || c == '\'' || c == '`')
}

/// Count the named children of a `call_expression`'s `arguments` field.
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

/// Short type name from a `generic_type`: drops type arguments.
fn generic_short(node: Node<'_>, source: &[u8]) -> Option<String> {
	let inner = node.child_by_field_name("name").or_else(|| {
		// fall back to first named child with a usable kind
		let mut cursor = node.walk();
		node.named_children(&mut cursor).next()
	})?;
	match inner.kind() {
		"type_identifier" => inner.utf8_text(source).ok().map(|s| s.to_string()),
		"nested_type_identifier" => nested_type_short(inner, source),
		_ => inner.utf8_text(source).ok().map(|s| s.to_string()),
	}
}

/// Short type name from a `nested_type_identifier`: keeps the rightmost
/// identifier (`Foo.Bar.Baz` → `Baz`).
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
