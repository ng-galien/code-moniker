//! Refs extraction: imports, calls, method_call (with receiver hint),
//! type uses, identifier reads.

use tree_sitter::Node;

use crate::core::code_graph::{CodeGraph, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{extend_callable_arity, extend_segment, node_position};
use super::kinds;
use super::walker::{last_attribute, Walker};

impl<'src> Walker<'src> {
	// --- imports ---------------------------------------------------------

	pub(super) fn handle_import(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let mut cursor = node.walk();
		let mut targets: Vec<Node<'_>> = Vec::new();
		for c in node.children(&mut cursor) {
			if matches!(c.kind(), "dotted_name" | "aliased_import") {
				targets.push(c);
			}
		}
		for t in targets {
			self.emit_import_module(t, scope, graph, node_position(node));
		}
	}

	fn emit_import_module(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
		pos: (u32, u32),
	) {
		let (path_node, alias) = match node.kind() {
			"aliased_import" => (
				node.child_by_field_name("name"),
				node.child_by_field_name("alias")
					.and_then(|n| n.utf8_text(self.source_bytes).ok())
					.unwrap_or(""),
			),
			_ => (Some(node), ""),
		};
		let Some(path_node) = path_node else { return };
		let pieces = dotted_pieces(path_node, self.source_bytes);
		if pieces.is_empty() {
			return;
		}
		let confidence = external_or_imported(&pieces);
		let bind = if !alias.is_empty() { alias } else { pieces[0] };
		self.imports
			.borrow_mut()
			.insert(bind.as_bytes(), confidence);

		let target = build_module_target(&self.module, &pieces, 0, confidence);
		let attrs = RefAttrs {
			confidence,
			alias: alias.as_bytes(),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
	}

	pub(super) fn handle_import_from(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut CodeGraph,
	) {
		let pos = node_position(node);
		let Some(module_node) = node.child_by_field_name("module_name") else { return };
		let (pieces, leading_dots) = match module_node.kind() {
			"relative_import" => relative_import_pieces(module_node, self.source_bytes),
			"dotted_name" => (dotted_pieces(module_node, self.source_bytes), 0),
			_ => return,
		};

		let mut wildcard = false;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "wildcard_import" {
				wildcard = true;
			}
		}
		let confidence = if leading_dots > 0 {
			kinds::CONF_IMPORTED
		} else {
			external_or_imported(&pieces)
		};
		let module_target =
			build_module_target(&self.module, &pieces, leading_dots, confidence);

		if wildcard {
			let attrs = RefAttrs { confidence, ..RefAttrs::default() };
			let _ = graph.add_ref_attrs(
				scope,
				module_target,
				kinds::IMPORTS_MODULE,
				Some(pos),
				&attrs,
			);
			return;
		}

		let names = collect_from_import_names(node, self.source_bytes);
		for (name, alias) in names {
			let bind: &'src str = if !alias.is_empty() { alias } else { name };
			self.imports
				.borrow_mut()
				.insert(bind.as_bytes(), confidence);
			let target = build_imported_symbol_target(
				&self.module,
				&pieces,
				leading_dots,
				name.as_bytes(),
				confidence,
			);
			let attrs = RefAttrs {
				confidence,
				alias: alias.as_bytes(),
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

	// --- calls -----------------------------------------------------------

	pub(super) fn handle_call(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		let pos = node_position(node);
		let Some(callee) = node.child_by_field_name("function") else {
			self.walk(node, scope, graph);
			return;
		};
		let arity = argument_count(node);

		match callee.kind() {
			"identifier" => {
				let name = self.text_of(callee);
				if !name.is_empty() {
					let target = extend_callable_arity(
						&self.module,
						kinds::FUNCTION,
						name.as_bytes(),
						arity,
					);
					let confidence = self
						.import_confidence_for(name.as_bytes())
						.unwrap_or_else(|| self.name_confidence(name.as_bytes()));
					let attrs = RefAttrs { confidence, ..RefAttrs::default() };
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::CALLS,
						Some(pos),
						&attrs,
					);
				}
			}
			"attribute" => {
				let name = last_attribute(callee, self.source_bytes);
				if !name.is_empty() {
					let target = extend_callable_arity(
						&self.module,
						kinds::METHOD,
						name.as_bytes(),
						arity,
					);
					let receiver = callee.child_by_field_name("object");
					let hint = receiver
						.map(|r| receiver_hint(r, self.source_bytes))
						.unwrap_or(b"");
					let attrs = RefAttrs {
						receiver_hint: hint,
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
				}
				if let Some(obj) = callee.child_by_field_name("object") {
					self.dispatch(obj, scope, graph);
				}
			}
			_ => self.dispatch(callee, scope, graph),
		}

		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk(args, scope, graph);
		}
	}

	// --- type uses -------------------------------------------------------

	pub(super) fn emit_uses_type(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
		// `type` parents an arbitrary expression; descend until we find
		// identifiers / attributes / subscripts.
		match node.kind() {
			"type" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					self.emit_uses_type(c, scope, graph);
				}
			}
			"identifier" => {
				let name = self.text_of(node);
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
			"attribute" => {
				let name = last_attribute(node, self.source_bytes);
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
			"subscript" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					if c.kind() != "slice" {
						self.emit_uses_type(c, scope, graph);
					}
				}
			}
			"generic_type" | "type_parameter" | "member_type" | "constrained_type"
			| "splat_type" | "tuple" | "list" => {
				let mut cursor = node.walk();
				for c in node.named_children(&mut cursor) {
					self.emit_uses_type(c, scope, graph);
				}
			}
			_ => {}
		}
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
		let target = extend_segment(&self.module, kinds::FUNCTION, name.as_bytes());
		let confidence = self
			.import_confidence_for(name.as_bytes())
			.unwrap_or_else(|| self.name_confidence(name.as_bytes()));
		let attrs = RefAttrs { confidence, ..RefAttrs::default() };
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::READS,
			Some(node_position(node)),
			&attrs,
		);
	}
}

/// Receiver shape for an `attribute`-callee call's `object`.
fn receiver_hint(obj: Node<'_>, source: &[u8]) -> &'static [u8] {
	match obj.kind() {
		"identifier" => match obj.utf8_text(source).unwrap_or("") {
			"self" => b"self",
			"cls" => b"cls",
			_ => b"identifier",
		},
		"attribute" => b"member",
		"call" => b"call",
		"subscript" => b"subscript",
		_ => b"",
	}
}

/// Pull `["a","b","c"]` from a `dotted_name` node.
fn dotted_pieces<'a>(node: Node<'_>, source: &'a [u8]) -> Vec<&'a str> {
	let mut out = Vec::new();
	let mut cursor = node.walk();
	for c in node.named_children(&mut cursor) {
		if c.kind() == "identifier" {
			if let Ok(s) = c.utf8_text(source) {
				out.push(s);
			}
		}
	}
	out
}

/// `from .foo.bar import X` → (`["foo", "bar"]`, leading_dots = 1).
fn relative_import_pieces<'a>(
	node: Node<'_>,
	source: &'a [u8],
) -> (Vec<&'a str>, usize) {
	let mut leading_dots = 0usize;
	let mut pieces: Vec<&str> = Vec::new();
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		match c.kind() {
			"import_prefix" => {
				if let Ok(s) = c.utf8_text(source) {
					leading_dots = s.chars().filter(|ch| *ch == '.').count();
				}
			}
			"dotted_name" => {
				pieces = dotted_pieces(c, source);
			}
			_ => {}
		}
	}
	(pieces, leading_dots)
}

/// Build the `(name, alias)` list for a `from X import a, b as c, d`
/// statement. Slices into `source` so the caller can use the strings
/// in `'src`-keyed maps without allocating.
fn collect_from_import_names<'src>(
	node: Node<'_>,
	source: &'src [u8],
) -> Vec<(&'src str, &'src str)> {
	let mut out: Vec<(&'src str, &'src str)> = Vec::new();
	let mut cursor = node.walk();
	for c in node.children_by_field_name("name", &mut cursor) {
		match c.kind() {
			"dotted_name" => {
				let leaf = dotted_leaf(c, source);
				if !leaf.is_empty() {
					out.push((leaf, ""));
				}
			}
			"aliased_import" => {
				let name_node = c.child_by_field_name("name");
				let alias = c
					.child_by_field_name("alias")
					.and_then(|n| n.utf8_text(source).ok())
					.unwrap_or("");
				let leaf = match name_node {
					Some(n) if n.kind() == "dotted_name" => dotted_leaf(n, source),
					Some(n) => n.utf8_text(source).unwrap_or(""),
					None => "",
				};
				if !leaf.is_empty() {
					out.push((leaf, alias));
				}
			}
			_ => {}
		}
	}
	out
}

/// Trailing identifier of a `dotted_name` node, sliced into `source`.
fn dotted_leaf<'src>(node: Node<'_>, source: &'src [u8]) -> &'src str {
	let mut cursor = node.walk();
	let mut last = "";
	for c in node.named_children(&mut cursor) {
		if c.kind() == "identifier" {
			if let Ok(s) = c.utf8_text(source) {
				last = s;
			}
		}
	}
	last
}

/// Build a module-target moniker for an import.
///
/// Three resolution modes:
/// - **Absolute project-local** (`confidence == imported`,
///   `leading_dots == 0`): build under the language regime,
///   `lang:python/package:<heads>/module:<leaf>`, so `bind_match`
///   can JOIN against the export-side def.
/// - **Relative** (`leading_dots > 0`): walk up the importer's module
///   chain by `leading_dots - 1` package segments, then attach
///   `pieces` as `package:` ... `module:<leaf>`. Targets are still in
///   the language regime, so `bind_match` works.
/// - **External / stdlib** (`confidence == external`): keep the
///   project-regime `external_pkg:<head>/path:<rest>` shape. There is
///   no project-side def to match against.
fn build_module_target(
	importer: &Moniker,
	pieces: &[&str],
	leading_dots: usize,
	confidence: &[u8],
) -> Moniker {
	let project = importer.as_view().project();
	if leading_dots > 0 {
		return build_relative_module_target(importer, pieces, leading_dots);
	}
	if pieces.is_empty() {
		let mut b = MonikerBuilder::new();
		b.project(project);
		return b.build();
	}
	if confidence == kinds::CONF_IMPORTED {
		let mut b = MonikerBuilder::new();
		b.project(project);
		b.segment(crate::lang::kinds::LANG, b"python");
		let last = pieces.len() - 1;
		for (i, p) in pieces.iter().enumerate() {
			let kind = if i == last { kinds::MODULE } else { kinds::PACKAGE };
			b.segment(kind, p.as_bytes());
		}
		return b.build();
	}
	// confidence == external (stdlib): project-regime external_pkg.
	let mut b = MonikerBuilder::new();
	b.project(project);
	b.segment(kinds::EXTERNAL_PKG, pieces[0].as_bytes());
	for p in &pieces[1..] {
		b.segment(kinds::PATH, p.as_bytes());
	}
	b.build()
}

/// Resolve a relative import against the importer's module moniker.
/// Python `.` semantics: `from .X import Y` references the current
/// package; each extra dot walks up one more package level. The
/// importer's module moniker is shaped
/// `.../lang:python/<package:>*/module:<leaf>`. We drop the trailing
/// `module:` segment to land at the importer's current package, then
/// drop `leading_dots - 1` further package segments, then append the
/// requested `pieces` as `package:` … `module:<leaf>`.
fn build_relative_module_target(
	importer: &Moniker,
	pieces: &[&str],
	leading_dots: usize,
) -> Moniker {
	let view = importer.as_view();
	let depth = view.segment_count() as usize;
	// Drop the importer's `module:` leaf, then `leading_dots - 1`
	// further parent package segments. Bottom out at zero — never
	// reach into the project regime.
	let keep = depth.saturating_sub(1).saturating_sub(leading_dots.saturating_sub(1));
	if keep == 0 {
		// Underflow: more dots than the importer has parents. Fall
		// back to a synthetic project-regime shape so the import is
		// at least diagnosable; bind_match never resolves it.
		let mut b = MonikerBuilder::new();
		b.project(view.project());
		let head = ".".repeat(leading_dots);
		b.segment(kinds::EXTERNAL_PKG, head.as_bytes());
		for p in pieces {
			b.segment(kinds::PATH, p.as_bytes());
		}
		return b.build();
	}
	let mut b = MonikerBuilder::from_view(view);
	b.truncate(keep);
	if pieces.is_empty() {
		return b.build();
	}
	let last = pieces.len() - 1;
	for (i, p) in pieces.iter().enumerate() {
		let kind = if i == last { kinds::MODULE } else { kinds::PACKAGE };
		b.segment(kind, p.as_bytes());
	}
	b.build()
}

/// Build a target for `from X import Y`. When the module target lives
/// in the language regime (project-local import or resolved relative
/// import), append `path:<name>` — matches the canonical def shape
/// modulo last-segment kind so `bind_match` resolves it. When
/// external, append the arity-only callable shape so downstream
/// projections can match by name+arity.
fn build_imported_symbol_target(
	importer: &Moniker,
	pieces: &[&str],
	leading_dots: usize,
	name: &[u8],
	confidence: &[u8],
) -> Moniker {
	let module = build_module_target(importer, pieces, leading_dots, confidence);
	let language_regime = leading_dots > 0
		|| (confidence == kinds::CONF_IMPORTED && !pieces.is_empty());
	if language_regime {
		extend_segment(&module, kinds::PATH, name)
	} else {
		extend_callable_arity(&module, kinds::FUNCTION, name, 0)
	}
}

fn external_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	if STDLIB_PACKAGES.binary_search(&pieces[0]).is_ok() {
		return kinds::CONF_EXTERNAL;
	}
	kinds::CONF_IMPORTED
}

/// Tiny stdlib head-package list. Anything outside this set lands as
/// `imported` (consumers may refine via presets / package metadata).
/// Keep alphabetically sorted for the binary search above.
const STDLIB_PACKAGES: &[&str] = &[
	"abc", "argparse", "ast", "asyncio", "base64", "collections",
	"concurrent", "contextlib", "copy", "csv", "dataclasses", "datetime",
	"decimal", "difflib", "enum", "errno", "functools", "gc", "glob",
	"hashlib", "heapq", "http", "importlib", "inspect", "io", "ipaddress",
	"itertools", "json", "logging", "math", "multiprocessing", "operator",
	"os", "pathlib", "pickle", "pkgutil", "platform", "pprint", "queue",
	"random", "re", "secrets", "shutil", "signal", "socket", "sqlite3",
	"ssl", "stat", "string", "struct", "subprocess", "sys", "tempfile",
	"textwrap", "threading", "time", "timeit", "traceback", "types",
	"typing", "unicodedata", "unittest", "urllib", "uuid", "warnings",
	"weakref", "xml", "zipfile",
];

fn argument_count(call: Node<'_>) -> u16 {
	let Some(args) = call.child_by_field_name("arguments") else { return 0 };
	let mut cursor = args.walk();
	let mut count: u16 = 0;
	for _ in args.named_children(&mut cursor) {
		count = count.saturating_add(1);
	}
	count
}
