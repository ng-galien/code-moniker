use crate::core::code_graph::CodeGraph;
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::ParsedDocument;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};
use tree_sitter::Node;

mod builtins;
mod defs;
mod discover;
mod imports;
mod lombok;
mod refs;
mod symbols;
mod syntax;
mod type_resolution;

use discover::JavaDiscover;

use super::Presets;
use super::kinds;

pub fn extract(
	uri: &str,
	source: &str,
	document: &ParsedDocument,
	anchor: &Moniker,
	deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let root = document.primary().root_node();
	let pkg = read_package_name(root, source.as_bytes());
	let pieces = pkg
		.split('.')
		.filter(|piece| !piece.is_empty())
		.collect::<Vec<_>>();
	let module = compute_module_moniker(anchor, uri, &pieces);
	let discovered_parts = JavaDiscover::run(module.clone(), source.as_bytes(), deep, root);
	let discovered = DiscoveredFile::new(
		module,
		kinds::MODULE,
		discovered_parts.defs,
		ScopeTree::new(discovered_parts.root),
		ImportTable::default(),
	);
	GraphEmitter::emit(&discovered, &discovered_parts.refs)
		.unwrap_or_else(|err| panic!("Java SDK graph emission failed: {err}"))
}

fn compute_module_moniker(anchor: &Moniker, uri: &str, package_pieces: &[&str]) -> Moniker {
	let class_name = file_stem(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(crate::lang::kinds::LANG, b"java");
	for piece in package_pieces.iter().filter(|piece| !piece.is_empty()) {
		builder.segment(kinds::PACKAGE, piece.as_bytes());
	}
	builder.segment(kinds::MODULE, class_name.as_bytes());
	builder.build()
}

pub(super) fn standard_path_module_moniker(anchor: &Moniker, uri: &str) -> Option<Moniker> {
	let pieces = uri
		.split(['/', '\\'])
		.filter(|piece| !piece.is_empty() && *piece != ".")
		.collect::<Vec<_>>();
	let java_idx = pieces
		.windows(3)
		.position(|parts| parts[0] == "src" && parts[2] == "java")?
		+ 2;
	let (_, tail) = pieces.split_at(java_idx + 1);
	let (file, package_pieces) = tail.split_last()?;
	let class_name = file_stem(file);
	if class_name.is_empty() {
		return None;
	}
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(crate::lang::kinds::LANG, b"java");
	for piece in package_pieces {
		builder.segment(kinds::PACKAGE, piece.as_bytes());
	}
	builder.segment(kinds::MODULE, class_name.as_bytes());
	Some(builder.build())
}

fn file_stem(uri: &str) -> &str {
	let after_slash = uri.rsplit(['/', '\\']).next().unwrap_or(uri);
	after_slash.strip_suffix(".java").unwrap_or(after_slash)
}

fn read_package_name<'src>(root: Node<'_>, source: &'src [u8]) -> &'src str {
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		if child.kind() != "package_declaration" {
			continue;
		}
		let mut named_cursor = child.walk();
		for name in child.named_children(&mut named_cursor) {
			if let Ok(package) = name.utf8_text(source) {
				return package;
			}
		}
	}
	""
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use super::*;

	#[test]
	fn standard_path_module_moniker_accepts_windows_separators() {
		let anchor = MonikerBuilder::new().project(b"app").build();
		let posix =
			standard_path_module_moniker(&anchor, "src/main/java/com/acme/java/util/Read.java")
				.expect("standard Java path");
		let windows_uri = r"src\main\java\com\acme\java\util\Read.java";
		let windows =
			standard_path_module_moniker(&anchor, windows_uri).expect("standard Windows Java path");
		let graph = crate::lang::java::extract(
			windows_uri,
			"package com.acme.java.util; class Read {}",
			&anchor,
			true,
			&Presets::default(),
		);

		assert_eq!(windows, posix);
		assert_eq!(&windows, graph.root());
	}

	#[test]
	fn local_types_are_defined_and_resolved_in_their_callable_scope() {
		let source = r#"
			package com.acme;
			class Records {
				void first() {
					record Local(int value) {}
					Local local = new Local(1);
				}
				void second() {
					record Local(String value) {}
					Local local = new Local("two");
				}
			}
		"#;
		let anchor = MonikerBuilder::new()
			.project(b"app")
			.segment(b"srcset", b"main")
			.build();
		let graph = crate::lang::java::extract(
			"src/main/java/com/acme/Records.java",
			source,
			&anchor,
			true,
			&Presets::default(),
		);
		let local_records = graph
			.defs()
			.filter(|def| {
				def.kind == kinds::RECORD
					&& def
						.moniker
						.as_view()
						.segments()
						.last()
						.is_some_and(|segment| segment.name == b"Local")
			})
			.map(|def| def.moniker.clone())
			.collect::<HashSet<_>>();

		assert_eq!(local_records.len(), 2, "{local_records:#?}");
		let local_type_targets = graph
			.refs()
			.filter(|reference| reference.kind == kinds::USES_TYPE)
			.filter(|reference| local_records.contains(&reference.target))
			.map(|reference| reference.target.clone())
			.collect::<HashSet<_>>();
		assert_eq!(local_type_targets, local_records);
	}
}
