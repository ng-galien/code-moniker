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
	let tree = walker::parse(source);
	let walker = walker::Walker {
		source,
		module: module.clone(),
		deep,
	};
	walker.walk(tree.root_node(), &module, &mut graph);
	graph
}

pub struct Lang;

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "sql";
	const ALLOWED_KINDS: &'static [&'static str] =
		&["function", "procedure", "view", "table", "schema"];
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &[];

	fn extract(
		uri: &str,
		source: &str,
		anchor: &Moniker,
		deep: bool,
		presets: &Self::Presets,
	) -> CodeGraph {
		extract(uri, source, anchor, deep, presets)
	}
}
