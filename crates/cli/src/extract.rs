use std::path::Path;

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::Lang;

pub fn extract(lang: Lang, source: &str, path: &Path) -> CodeGraph {
	let uri = path.to_str().unwrap_or("single-file");
	let anchor = anchor_moniker();
	let deep = true;
	match lang {
		Lang::Ts => code_moniker_core::lang::ts::extract(
			uri,
			source,
			&anchor,
			deep,
			&code_moniker_core::lang::ts::Presets::default(),
		),
		Lang::Rs => code_moniker_core::lang::rs::extract(
			uri,
			source,
			&anchor,
			deep,
			&code_moniker_core::lang::rs::Presets::default(),
		),
		Lang::Java => code_moniker_core::lang::java::extract(
			uri,
			source,
			&anchor,
			deep,
			&code_moniker_core::lang::java::Presets::default(),
		),
		Lang::Python => code_moniker_core::lang::python::extract(
			uri,
			source,
			&anchor,
			deep,
			&code_moniker_core::lang::python::Presets::default(),
		),
		Lang::Go => code_moniker_core::lang::go::extract(
			uri,
			source,
			&anchor,
			deep,
			&code_moniker_core::lang::go::Presets::default(),
		),
		Lang::Cs => code_moniker_core::lang::cs::extract(
			uri,
			source,
			&anchor,
			deep,
			&code_moniker_core::lang::cs::Presets::default(),
		),
		Lang::Sql => code_moniker_core::lang::sql::extract(
			uri,
			source,
			&anchor,
			deep,
			&code_moniker_core::lang::sql::Presets::default(),
		),
	}
}

fn anchor_moniker() -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(b".");
	b.build()
}

pub fn file_uri(path: &Path) -> String {
	let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
	format!("file://{}", abs.display())
}
