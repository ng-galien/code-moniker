use std::path::Path;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::Lang;

pub fn extract(lang: Lang, source: &str, path: &Path) -> CodeGraph {
	let uri = path.to_str().unwrap_or("single-file");
	let anchor = anchor_moniker();
	let deep = true;
	match lang {
		Lang::Ts => crate::lang::ts::extract(
			uri,
			source,
			&anchor,
			deep,
			&crate::lang::ts::Presets::default(),
		),
		Lang::Rs => crate::lang::rs::extract(
			uri,
			source,
			&anchor,
			deep,
			&crate::lang::rs::Presets::default(),
		),
		Lang::Java => crate::lang::java::extract(
			uri,
			source,
			&anchor,
			deep,
			&crate::lang::java::Presets::default(),
		),
		Lang::Python => crate::lang::python::extract(
			uri,
			source,
			&anchor,
			deep,
			&crate::lang::python::Presets::default(),
		),
		Lang::Go => crate::lang::go::extract(
			uri,
			source,
			&anchor,
			deep,
			&crate::lang::go::Presets::default(),
		),
		Lang::Cs => crate::lang::cs::extract(
			uri,
			source,
			&anchor,
			deep,
			&crate::lang::cs::Presets::default(),
		),
		Lang::Sql => crate::lang::sql::extract(
			uri,
			source,
			&anchor,
			deep,
			&crate::lang::sql::Presets::default(),
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
