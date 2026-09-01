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

pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str, language: &[u8]) -> Moniker {
	canonicalize::compute_module_moniker(anchor, uri, language)
}

pub fn extract(
	uri: &str,
	source: &str,
	document: &ParsedDocument,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri, b"ts");
	extract_module(module, source, document, anchor, deep, presets)
}

pub fn extract_tsx(
	uri: &str,
	source: &str,
	document: &ParsedDocument,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri, b"tsx");
	extract_module(module, source, document, anchor, deep, presets)
}

pub fn extract_js(
	uri: &str,
	source: &str,
	document: &ParsedDocument,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri, b"js");
	extract_module(module, source, document, anchor, deep, presets)
}

pub fn extract_jsx(
	uri: &str,
	source: &str,
	document: &ParsedDocument,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri, b"jsx");
	extract_module(module, source, document, anchor, deep, presets)
}

fn extract_module(
	module: Moniker,
	source: &str,
	document: &ParsedDocument,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
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
