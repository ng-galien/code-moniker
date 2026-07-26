use std::path::Path;

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::{Lang, ts};

use crate::cbuild::CBuildContext;
use crate::tsconfig::TsResolution;

#[derive(Debug, Clone, Default)]
pub struct Context {
	pub c: CBuildContext,
	pub ts: TsResolution,
	pub project: Option<String>,
}

pub fn extract(lang: Lang, source: &str, path: &Path) -> CodeGraph {
	extract_with(lang, source, path, &Context::default())
}

pub fn source_root(lang: Lang, path: &Path, ctx: &Context) -> Option<Moniker> {
	let uri = path.to_str()?;
	let anchor = path_anchor(path, ctx);
	lang.file_root(uri, &anchor)
}

pub fn extract_with(lang: Lang, source: &str, path: &Path, ctx: &Context) -> CodeGraph {
	let uri = path.to_str().unwrap_or("single-file");
	let anchor = path_anchor(path, ctx);
	let deep = true;
	let mut graph = match lang {
		Lang::Ts => {
			let presets = ts::Presets {
				path_aliases: ctx.ts.aliases.clone(),
				..ts::Presets::default()
			};
			ts::extract(uri, source, &anchor, deep, &presets)
		}
		Lang::Rs => code_moniker_core::lang::rs::extract_sdk(
			uri,
			source,
			&anchor,
			deep,
			&code_moniker_core::lang::rs::Presets::default(),
		),
		Lang::Java => code_moniker_core::lang::java::extract_sdk(
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
		Lang::C => {
			let presets = ctx.c.extraction_presets();
			code_moniker_core::lang::c::extract(uri, source, &anchor, deep, &presets)
		}
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
	};
	graph.shrink_to_fit();
	graph
}

fn path_anchor(path: &Path, ctx: &Context) -> Moniker {
	let project = ctx.project.as_deref().map(str::as_bytes).unwrap_or(b".");
	anchor_moniker(project, srcset(path).map(str::as_bytes))
}

fn anchor_moniker(project: &[u8], srcset: Option<&[u8]>) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(project);
	if let Some(srcset) = srcset {
		b.segment(b"srcset", srcset);
	}
	b.build()
}

fn srcset(path: &Path) -> Option<&'static str> {
	let parts: Vec<_> = path
		.components()
		.filter_map(|component| component.as_os_str().to_str())
		.collect();
	for window in parts.windows(2) {
		match window {
			["src", "main"] => return Some("main"),
			["src", "test"] | ["src", "tests"] => return Some("test"),
			_ => {}
		}
	}
	None
}

pub fn file_uri(path: &Path) -> String {
	let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
	format!("file://{}", abs.display())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn path_derived_roots_match_extracted_graph_roots() {
		let cases = [
			(Lang::Rs, "src/tools/mod.rs", ""),
			(Lang::Ts, "src/tools/read.ts", ""),
			(Lang::Python, "src/tools/read.py", ""),
			(Lang::Go, "src/tools/read.go", ""),
			(Lang::C, "src/tools/read.c", ""),
			(Lang::Cs, "src/tools/Read.cs", ""),
			(Lang::Sql, "db/tools/read.sql", ""),
			(
				Lang::Java,
				"src/main/java/app/tools/Read.java",
				"package app.tools; class Read {}",
			),
		];
		let ctx = Context::default();
		for (lang, path, source) in cases {
			let path = Path::new(path);
			let catalog_root = source_root(lang, path, &ctx)
				.unwrap_or_else(|| panic!("{} should expose a path root", lang.tag()));
			let graph = extract_with(lang, source, path, &ctx);
			assert_eq!(
				&catalog_root,
				graph.root(),
				"{} catalog root drifted from extraction",
				lang.tag()
			);
		}
	}

	#[test]
	fn java_path_root_requires_a_standard_source_root() {
		let ctx = Context::default();
		assert!(source_root(Lang::Java, Path::new("fixtures/app/tools/Read.java"), &ctx).is_none());
	}

	#[test]
	fn java_path_root_keeps_java_package_segments_after_the_source_root() {
		let ctx = Context::default();
		let path = Path::new("src/main/java/com/acme/java/util/Read.java");
		let source = "package com.acme.java.util; class Read {}";
		let catalog_root = source_root(Lang::Java, path, &ctx).expect("standard Java path");
		let graph = extract_with(Lang::Java, source, path, &ctx);

		assert_eq!(&catalog_root, graph.root());
	}

	#[test]
	fn java_path_root_accepts_test_and_custom_source_sets() {
		let ctx = Context::default();
		for path in [
			"src/test/java/app/tools/Read.java",
			"src/integrationTest/java/app/tools/Read.java",
		] {
			let path = Path::new(path);
			let catalog_root =
				source_root(Lang::Java, path, &ctx).expect("standard Java source root");
			let graph = extract_with(Lang::Java, "package app.tools; class Read {}", path, &ctx);

			assert_eq!(&catalog_root, graph.root());
		}
	}

	#[test]
	fn java_path_root_exposes_package_misalignment() {
		let ctx = Context::default();
		let path = Path::new("src/main/java/app/tools/Read.java");
		let catalog_root = source_root(Lang::Java, path, &ctx).expect("standard Java path");
		let graph = extract_with(Lang::Java, "package other; class Read {}", path, &ctx);
		assert_ne!(
			&catalog_root,
			graph.root(),
			"a declared package mismatch must remain observable"
		);
	}
}
