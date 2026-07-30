//! SDK-native SQL extraction pipeline.

pub(super) mod discover;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::ParsedDocument;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

use super::Presets;
use super::canonicalize::compute_module_moniker;
use super::kinds;
use discover::SqlDiscover;

pub(super) fn extract(
	uri: &str,
	source: &str,
	document: &ParsedDocument,
	anchor: &Moniker,
	_deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let parts = SqlDiscover::run(module, source, document);
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
