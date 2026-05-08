use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{extend_callable_arity, extend_segment, node_position};
use super::kinds;
use super::walker::{ImportEntry, Walker};

impl<'src> Walker<'src> {
	pub(super) fn handle_import(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			match child.kind() {
				"import_spec" => self.handle_import_spec(child, scope, graph),
				"import_spec_list" => {
					let mut sc = child.walk();
					for spec in child.named_children(&mut sc) {
						if spec.kind() == "import_spec" {
							self.handle_import_spec(spec, scope, graph);
						}
					}
				}
				_ => {}
			}
		}
	}

	fn handle_import_spec(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let Some(path_node) = node.child_by_field_name("path") else {
			return;
		};
		let raw = self.text_of(path_node);
		let path = strip_string_quotes(raw);
		if path.is_empty() {
			return;
		}
		let pieces: Vec<&'src str> = path.split('/').filter(|s| !s.is_empty()).collect();
		if pieces.is_empty() {
			return;
		}

		let alias_text: &'src str = node
			.child_by_field_name("name")
			.map(|n| self.text_of(n))
			.unwrap_or("");

		let confidence = stdlib_or_imported(&pieces);

		let bind: Option<&'src [u8]> = match alias_text {
			"" => pieces.last().copied().map(str::as_bytes),
			"." | "_" => None,
			other => Some(other.as_bytes()),
		};
		let module_prefix = build_module_target(self.module.as_view().project(), &pieces);
		if let Some(b) = bind
			&& !b.is_empty()
		{
			self.imports.borrow_mut().insert(
				b,
				ImportEntry {
					confidence,
					module_prefix: module_prefix.clone(),
				},
			);
		}

		let attrs = RefAttrs {
			confidence,
			alias: alias_text.as_bytes(),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			module_prefix,
			kinds::IMPORTS_MODULE,
			Some(node_position(node)),
			&attrs,
		);
	}

	pub(super) fn handle_call(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let arity = argument_count(node);
		let Some(callee) = node.child_by_field_name("function") else {
			self.walk(node, scope, graph);
			return;
		};
		match callee.kind() {
			"identifier" => self.emit_simple_call(callee, scope, arity, pos, graph),
			"selector_expression" => self.emit_selector_call(callee, scope, arity, pos, graph),
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

	fn emit_selector_call(
		&self,
		callee: Node<'_>,
		scope: &Moniker,
		arity: u16,
		pos: (u32, u32),
		graph: &mut CodeGraph,
	) {
		let Some(field_node) = callee.child_by_field_name("field") else {
			self.walk(callee, scope, graph);
			return;
		};
		let name = self.text_of(field_node);
		if name.is_empty() {
			return;
		}
		let operand = callee.child_by_field_name("operand");

		if let Some(op) = operand
			&& op.kind() == "identifier"
		{
			let op_name = self.text_of(op);
			if let Some(entry) = self.import_entry_for(op_name.as_bytes()) {
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
		}

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

	fn resolve_type_node(&self, type_node: Node<'_>) -> Option<(Moniker, &'static [u8])> {
		match type_node.kind() {
			"type_identifier" => {
				let name = self.text_of(type_node);
				if name.is_empty() {
					return None;
				}
				Some(self.resolve_type_target(name.as_bytes(), kinds::CLASS))
			}
			"qualified_type" => {
				let pkg = type_node
					.child_by_field_name("package")
					.map(|n| self.text_of(n))
					.unwrap_or("");
				let name_node = type_node.child_by_field_name("name")?;
				let name = self.text_of(name_node);
				if name.is_empty() {
					return None;
				}
				if let Some(entry) = self.import_entry_for(pkg.as_bytes()) {
					Some((
						extend_segment(&entry.module_prefix, kinds::CLASS, name.as_bytes()),
						entry.confidence,
					))
				} else {
					Some(self.resolve_type_target(name.as_bytes(), kinds::CLASS))
				}
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

	pub(super) fn emit_uses_type(
		&self,
		type_node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		match type_node.kind() {
			"type_identifier" | "qualified_type" => {
				self.emit_resolved_type_ref(
					type_node,
					scope,
					kinds::USES_TYPE,
					node_position(type_node),
					graph,
				);
			}
			"pointer_type" | "slice_type" | "array_type" | "channel_type" | "map_type"
			| "parenthesized_type" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					self.emit_uses_type(c, scope, graph);
				}
			}
			"generic_type" => {
				if let Some(head) = type_node.child_by_field_name("type") {
					self.emit_uses_type(head, scope, graph);
				}
				if let Some(args) = type_node.child_by_field_name("type_arguments") {
					let mut cursor = args.walk();
					for c in args.named_children(&mut cursor) {
						self.emit_uses_type(c, scope, graph);
					}
				}
			}
			"function_type" => {
				if let Some(params) = type_node.child_by_field_name("parameters") {
					let mut cursor = params.walk();
					for c in params.named_children(&mut cursor) {
						if let Some(t) = c.child_by_field_name("type") {
							self.emit_uses_type(t, scope, graph);
						}
					}
				}
				if let Some(result) = type_node.child_by_field_name("result") {
					self.emit_uses_type(result, scope, graph);
				}
			}
			"parameter_list" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					if let Some(t) = c.child_by_field_name("type") {
						self.emit_uses_type(t, scope, graph);
					}
				}
			}
			"struct_type" | "interface_type" => {
				self.walk(type_node, scope, graph);
			}
			_ => {}
		}
	}

	pub(super) fn emit_callable_type_refs(
		&self,
		callable: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		if let Some(params) = callable.child_by_field_name("parameters") {
			let mut cursor = params.walk();
			for c in params.named_children(&mut cursor) {
				if let Some(t) = c.child_by_field_name("type") {
					self.emit_uses_type(t, scope, graph);
				}
			}
		}
		if let Some(result) = callable.child_by_field_name("result") {
			self.emit_uses_type(result, scope, graph);
		}
	}

	pub(super) fn emit_struct_body(
		&self,
		struct_node: Node<'_>,
		owner: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(field_list) = struct_field_list(struct_node) else {
			return;
		};
		let mut cursor = field_list.walk();
		for field in field_list.named_children(&mut cursor) {
			if field.kind() != "field_declaration" {
				continue;
			}
			let Some(type_node) = field.child_by_field_name("type") else {
				continue;
			};
			if field.child_by_field_name("name").is_some() {
				self.emit_uses_type(type_node, owner, graph);
			} else {
				self.emit_extends(type_node, owner, graph);
			}
		}
	}

	pub(super) fn emit_interface_body(
		&self,
		interface_node: Node<'_>,
		owner: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = interface_node.walk();
		for child in interface_node.named_children(&mut cursor) {
			match child.kind() {
				"method_elem" => {
					self.emit_callable_type_refs(child, owner, graph);
				}
				"type_elem" => {
					let mut tc = child.walk();
					for t in child.named_children(&mut tc) {
						self.emit_extends(t, owner, graph);
					}
				}
				_ => {}
			}
		}
	}

	fn emit_extends(&self, type_node: Node<'_>, owner: &Moniker, graph: &mut CodeGraph) {
		match type_node.kind() {
			"type_identifier" | "qualified_type" => {
				self.emit_resolved_type_ref(
					type_node,
					owner,
					kinds::EXTENDS,
					node_position(type_node),
					graph,
				);
			}
			"pointer_type" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					self.emit_extends(c, owner, graph);
				}
			}
			"generic_type" => {
				if let Some(head) = type_node.child_by_field_name("type") {
					self.emit_extends(head, owner, graph);
				}
			}
			_ => {}
		}
	}

	pub(super) fn handle_composite_literal(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let pos = node_position(node);
		if let Some(type_node) = node.child_by_field_name("type") {
			self.emit_instantiates(type_node, scope, pos, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk(body, scope, graph);
		}
	}

	fn emit_instantiates(
		&self,
		type_node: Node<'_>,
		scope: &Moniker,
		pos: (u32, u32),
		graph: &mut CodeGraph,
	) {
		match type_node.kind() {
			"type_identifier" | "qualified_type" => {
				self.emit_resolved_type_ref(type_node, scope, kinds::INSTANTIATES, pos, graph);
			}
			"generic_type" => {
				if let Some(inner) = type_node.child_by_field_name("type") {
					self.emit_instantiates(inner, scope, pos, graph);
				}
			}
			_ => {}
		}
	}
}

fn strip_string_quotes(raw: &str) -> &str {
	let trimmed = raw
		.strip_prefix('"')
		.and_then(|s| s.strip_suffix('"'))
		.or_else(|| raw.strip_prefix('`').and_then(|s| s.strip_suffix('`')));
	trimmed.unwrap_or(raw)
}

fn stdlib_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	if STDLIB_PACKAGES.binary_search(&pieces[0]).is_ok() {
		return kinds::CONF_EXTERNAL;
	}
	kinds::CONF_IMPORTED
}

fn build_module_target(project: &[u8], pieces: &[&str]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(project);
	b.segment(kinds::EXTERNAL_PKG, pieces[0].as_bytes());
	for p in &pieces[1..] {
		b.segment(kinds::PATH, p.as_bytes());
	}
	b.build()
}

fn struct_field_list<'tree>(struct_node: Node<'tree>) -> Option<Node<'tree>> {
	let mut cursor = struct_node.walk();
	struct_node
		.named_children(&mut cursor)
		.find(|&c| c.kind() == "field_declaration_list")
}

fn argument_count(call: Node<'_>) -> u16 {
	let Some(args) = call.child_by_field_name("arguments") else {
		return 0;
	};
	let mut cursor = args.walk();
	let mut count: u16 = 0;
	for _ in args.named_children(&mut cursor) {
		count = count.saturating_add(1);
	}
	count
}

fn receiver_hint<'a>(obj: Node<'_>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SUBSCRIPT};
	match obj.kind() {
		"identifier" => obj.utf8_text(source).unwrap_or("").as_bytes(),
		"selector_expression" | "field_identifier" => HINT_MEMBER,
		"call_expression" => HINT_CALL,
		"index_expression" => HINT_SUBSCRIPT,
		_ => b"",
	}
}

const STDLIB_PACKAGES: &[&str] = &[
	"archive",
	"bufio",
	"builtin",
	"bytes",
	"cmp",
	"compress",
	"container",
	"context",
	"crypto",
	"database",
	"debug",
	"embed",
	"encoding",
	"errors",
	"expvar",
	"flag",
	"fmt",
	"go",
	"hash",
	"html",
	"image",
	"index",
	"io",
	"iter",
	"log",
	"maps",
	"math",
	"mime",
	"net",
	"os",
	"path",
	"plugin",
	"reflect",
	"regexp",
	"runtime",
	"slices",
	"sort",
	"strconv",
	"strings",
	"sync",
	"syscall",
	"testing",
	"text",
	"time",
	"unicode",
	"unsafe",
];

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn strip_quotes_handles_double_and_backtick() {
		assert_eq!(strip_string_quotes("\"fmt\""), "fmt");
		assert_eq!(strip_string_quotes("`net/http`"), "net/http");
		assert_eq!(strip_string_quotes("fmt"), "fmt");
	}

	#[test]
	fn stdlib_detection_known_packages() {
		assert_eq!(stdlib_or_imported(&["fmt"]), kinds::CONF_EXTERNAL);
		assert_eq!(stdlib_or_imported(&["net", "http"]), kinds::CONF_EXTERNAL);
		assert_eq!(
			stdlib_or_imported(&["github.com", "foo"]),
			kinds::CONF_IMPORTED
		);
	}

	#[test]
	fn stdlib_packages_list_is_sorted() {
		let mut sorted = STDLIB_PACKAGES.to_vec();
		sorted.sort();
		assert_eq!(sorted, STDLIB_PACKAGES);
	}
}
