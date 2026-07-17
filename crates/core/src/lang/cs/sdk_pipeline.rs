//! SDK-native C# extraction pipeline.

pub(super) mod discover;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

use super::canonicalize::compute_module_moniker;
use super::kinds;
use discover::discover;

pub(super) fn extract(uri: &str, source: &str, anchor: &Moniker, deep: bool) -> CodeGraph {
	let tree = super::parse(source);
	let module = compute_module_moniker(anchor, uri);
	let parts = discover(module, source, tree.root_node(), deep);
	let discovered = DiscoveredFile::new(
		parts.root.clone(),
		kinds::MODULE,
		parts.defs,
		ScopeTree::new(parts.root),
		ImportTable::default(),
	);
	GraphEmitter::emit(&discovered, &parts.refs)
		.unwrap_or_else(|err| panic!("C# SDK graph emission failed: {err}"))
}
