use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

mod defs;
mod discover;
mod imports;
mod refs;
mod syntax;

use discover::JavaDiscover;

use super::canonicalize::{compute_module_moniker, read_package_name};
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
