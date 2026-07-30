use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::ParsedDocument;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

mod discover;
mod local_types;

use discover::PyDiscover;

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
	let module = super::canonicalize::compute_module_moniker(anchor, uri);
	let discovered_parts = PyDiscover::run(
		module.clone(),
		source.as_bytes(),
		deep,
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
		.unwrap_or_else(|err| panic!("Python SDK graph emission failed: {err}"))
}
