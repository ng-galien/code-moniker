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
	GraphEmitter::emit(&discovered, &discovered_parts.refs)
		.unwrap_or_else(|err| panic!("Rust SDK graph emission failed: {err}"))
}

fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let stem = uri.strip_suffix(".rs").unwrap_or(uri);
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(crate::lang::kinds::LANG, b"rs");
	append_rust_module_path(&mut builder, stem);
	builder.build()
}

fn append_rust_module_path(builder: &mut MonikerBuilder, path: &str) {
	let pieces = path
		.split('/')
		.filter(|piece| !piece.is_empty() && *piece != ".")
		.collect::<Vec<_>>();
	let Some(src_idx) = pieces.iter().rposition(|piece| *piece == "src") else {
		crate::lang::callable::append_dir_module_segments(builder, path, kinds::DIR, kinds::MODULE);
		return;
	};
	for piece in &pieces[..=src_idx] {
		builder.segment(kinds::DIR, piece.as_bytes());
	}
	let mut modules = &pieces[src_idx + 1..];
	if modules.last().copied() == Some("mod") {
		modules = &modules[..modules.len().saturating_sub(1)];
	}
	for piece in modules {
		builder.segment(kinds::MODULE, piece.as_bytes());
	}
}
