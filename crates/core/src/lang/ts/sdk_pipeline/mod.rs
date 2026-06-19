use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

mod canonicalize;
mod defs;
mod discover;
mod refs;
mod syntax;

use discover::TsDiscover;

use super::kinds;
use super::{Presets, parse_with_uri};

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let module = canonicalize::compute_module_moniker(anchor, uri);
	let tree = parse_with_uri(source, uri);
	let discovered_parts = TsDiscover::run(
		module.clone(),
		anchor.clone(),
		source.as_bytes(),
		deep,
		presets,
		tree.root_node(),
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
