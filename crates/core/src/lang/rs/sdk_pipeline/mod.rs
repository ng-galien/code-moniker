use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

mod defs;
mod discover;
mod imports;
mod refs;
mod syntax;

use discover::RustDiscover;

use super::canonicalize::compute_module_moniker;
use super::kinds;
use super::{Presets, parse};

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let tree = parse(source);
	let discovered_parts =
		RustDiscover::run(module.clone(), source.as_bytes(), deep, tree.root_node());
	let discovered = DiscoveredFile::new(
		module,
		kinds::MODULE,
		discovered_parts.defs,
		ScopeTree::new(discovered_parts.root),
		ImportTable::default(),
	);
	GraphEmitter::emit(&discovered, &discovered_parts.refs).expect("Rust SDK graph emission")
}
