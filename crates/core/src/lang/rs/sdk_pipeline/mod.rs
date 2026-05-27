use crate::core::code_graph::CodeGraph;
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

mod defs;
mod discover;
mod imports;
mod refs;
mod syntax;

use discover::RustDiscover;

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

fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let stem = uri.strip_suffix(".rs").unwrap_or(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(crate::lang::kinds::LANG, b"rs");
	crate::lang::callable::append_dir_module_segments(
		&mut builder,
		stem,
		kinds::DIR,
		kinds::MODULE,
	);
	builder.build()
}
