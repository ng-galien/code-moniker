use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::ParsedDocument;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

mod canonicalize;
mod defs;
mod discover;
mod refs;
mod syntax;

use discover::TsDiscover;

use super::Presets;
use super::kinds;

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	canonicalize::compute_module_moniker(anchor, uri)
}

pub fn extract(
	uri: &str,
	source: &str,
	document: &ParsedDocument,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let discovered_parts = TsDiscover::run(
		module.clone(),
		anchor.clone(),
		source.as_bytes(),
		deep,
		presets,
		document.primary().root_node(),
	);
	let discovered = DiscoveredFile::new(
		module,
		kinds::MODULE,
		discovered_parts.defs,
		ScopeTree::new(discovered_parts.root),
		ImportTable::default(),
	);
	GraphEmitter::emit(&discovered, &discovered_parts.refs)
		.unwrap_or_else(|err| panic!("TypeScript SDK graph emission failed: {err}"))
}
