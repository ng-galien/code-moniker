use crate::core::code_graph::CodeGraph;
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::sdk::{DiscoveredFile, GraphEmitter, ImportTable, ScopeTree};

mod builtins;
mod defs;
mod discover;
mod imports;
mod refs;
mod syntax;
mod type_resolution;

use discover::GoDiscover;

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
	let module = compute_module_moniker(anchor, uri);
	let discovered_parts =
		GoDiscover::run(module.clone(), source.as_bytes(), deep, tree.root_node());
	let discovered = DiscoveredFile::new(
		module,
		kinds::MODULE,
		discovered_parts.defs,
		ScopeTree::new(discovered_parts.root),
		ImportTable::default(),
	);
	GraphEmitter::emit(&discovered, &discovered_parts.refs)
		.unwrap_or_else(|err| panic!("Go SDK graph emission failed: {err}"))
}

fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let stem = uri.strip_suffix(".go").unwrap_or(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(crate::lang::kinds::LANG, b"go");
	crate::lang::callable::append_dir_module_segments(
		&mut builder,
		stem,
		kinds::PACKAGE,
		kinds::MODULE,
	);
	builder.build()
}

pub(crate) use builtins::STDLIB_PACKAGES;
