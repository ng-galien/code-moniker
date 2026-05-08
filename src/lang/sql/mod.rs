mod body;
mod canonicalize;
mod kinds;
mod refs;
mod scope;
mod walker;

use canonicalize::compute_module_moniker;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

#[derive(Clone, Debug, Default)]
pub struct Presets {
	pub external_schemas: Vec<String>,
}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let mut graph = CodeGraph::new(module.clone(), kinds::MODULE);
	walker::walk_source(source, &module, deep, &mut graph);
	graph
}
