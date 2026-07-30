use crate::core::code_graph::CodeGraph;
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::ParsedDocument;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

mod builtins;
mod defs;
mod discover;
mod imports;
mod refs;
mod syntax;
mod type_resolution;

use discover::CDiscover;

use super::Presets;
use super::kinds;

pub fn extract(
	uri: &str,
	source: &str,
	document: &ParsedDocument,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let discovered_parts = CDiscover::run(
		module.clone(),
		source.as_bytes(),
		deep,
		document.primary().root_node(),
		presets,
	);
	let discovered = DiscoveredFile::new(
		module,
		kinds::MODULE,
		discovered_parts.defs,
		ScopeTree::new(discovered_parts.root),
		ImportTable::default(),
	);
	GraphEmitter::emit(&discovered, &discovered_parts.refs)
		.unwrap_or_else(|err| panic!("C SDK graph emission failed: {err}"))
}

// `.c` files drop their extension; headers keep `.h` in the module name so a
// pair like server.c / server.h never collides on one moniker.
pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let stem = uri.strip_suffix(".c").unwrap_or(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(crate::lang::kinds::LANG, b"c");
	crate::lang::callable::append_dir_module_segments(
		&mut builder,
		stem,
		kinds::DIR,
		kinds::MODULE,
	);
	builder.build()
}
