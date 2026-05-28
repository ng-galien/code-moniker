use crate::core::code_graph::CodeGraph;
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};
use tree_sitter::Node;

mod builtins;
mod defs;
mod discover;
mod imports;
mod refs;
mod syntax;
mod type_resolution;

use discover::JavaDiscover;

use super::kinds;
use super::{Presets, parse};

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let tree = parse(source);
	let pkg = read_package_name(tree.root_node(), source.as_bytes());
	let pieces = pkg
		.split('.')
		.filter(|piece| !piece.is_empty())
		.collect::<Vec<_>>();
	let module = compute_module_moniker(anchor, uri, &pieces);
	let discovered_parts =
		JavaDiscover::run(module.clone(), source.as_bytes(), deep, tree.root_node());
	let discovered = DiscoveredFile::new(
		module,
		kinds::MODULE,
		discovered_parts.defs,
		ScopeTree::new(discovered_parts.root),
		ImportTable::default(),
	);
	GraphEmitter::emit(&discovered, &discovered_parts.refs).expect("Java SDK graph emission")
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

fn file_stem(uri: &str) -> &str {
	let after_slash = uri.rsplit('/').next().unwrap_or(uri);
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
