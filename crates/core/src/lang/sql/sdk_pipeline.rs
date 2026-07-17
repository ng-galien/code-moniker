//! SDK-native SQL extraction pipeline.

pub(super) mod discover;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

use super::Presets;
use super::canonicalize::compute_module_moniker;
use super::kinds;
use discover::{SqlDiscover, parse};

pub(super) fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	_deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let tree = parse(source);
	let module = compute_module_moniker(anchor, uri);
	let parts = SqlDiscover::run(module, source, tree.root_node());
	let discovered = DiscoveredFile::new(
		parts.root.clone(),
		kinds::MODULE,
		parts.defs,
		ScopeTree::new(parts.root),
		ImportTable::default(),
	);
	GraphEmitter::emit(&discovered, &parts.refs)
		.unwrap_or_else(|err| panic!("SQL SDK graph emission failed: {err}"))
}
