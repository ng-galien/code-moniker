
use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{
	append_module_segments, external_pkg_builder, node_position, strip_known_extension,
};
use super::kinds;
use super::walker::Walker;

impl<'src> Walker<'src> {
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

		let confidence = import_confidence(&raw_spec);
		let Some(clause) = clause else {
			let target = self.import_module_target(&raw_spec);
			let attrs = RefAttrs { confidence, ..RefAttrs::default() };
			let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
			return;
		};

		let mut cursor = clause.walk();
		for c in clause.children(&mut cursor) {
			match c.kind() {
				"identifier" => {
					let local_name = self.text_of(c);
					let target = self.import_symbol_target(&raw_spec, "default");
					let attrs = RefAttrs {
						alias: local_name.as_bytes(),
						confidence,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::IMPORTS_SYMBOL,
						Some(pos),
						&attrs,
					);
				}
				"namespace_import" => {
					let alias = first_identifier_text(c, self.source_bytes);
					let target = self.import_module_target(&raw_spec);
					let attrs = RefAttrs {
						alias: alias.as_bytes(),
						confidence,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::IMPORTS_MODULE,
						Some(pos),
						&attrs,
					);
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
						let alias = spec
							.child_by_field_name("alias")
							.map(|n| self.text_of(n))
							.unwrap_or("");
						let target = self.import_symbol_target(&raw_spec, name);
						let attrs = RefAttrs {
							alias: alias.as_bytes(),
							confidence,
							..RefAttrs::default()
						};
						let _ = graph.add_ref_attrs(
							scope,
							target,
							kinds::IMPORTS_SYMBOL,
							Some(pos),
							&attrs,
						);
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

		let confidence = import_confidence(&raw_spec);
		if has_star {
			let target = self.import_module_target(&raw_spec);
			let attrs = RefAttrs { confidence, ..RefAttrs::default() };
			let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
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
			let alias = spec
				.child_by_field_name("alias")
				.map(|n| self.text_of(n))
				.unwrap_or("");
			let target = self.import_symbol_target(&raw_spec, name);
			let attrs = RefAttrs {
				alias: alias.as_bytes(),
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
		}
	}

	fn import_module_target(&self, raw_path: &str) -> Moniker {
		self.import_target(raw_path, None)
	}

	fn import_symbol_target(&self, raw_path: &str, name: &str) -> Moniker {
		self.import_target(raw_path, Some(name))
	}

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
		append_module_segments(&mut b, remainder);
		b
	}
}

fn is_relative_specifier(spec: &str) -> bool {
	spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../")
}

fn import_confidence(spec: &str) -> &'static [u8] {
	if is_relative_specifier(spec) {
		kinds::CONF_IMPORTED
	} else {
		kinds::CONF_EXTERNAL
	}
}

fn first_identifier_text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "identifier" {
			if let Ok(s) = c.utf8_text(source) {
				return s;
			}
		}
	}
	""
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
