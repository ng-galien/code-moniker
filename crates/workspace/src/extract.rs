use std::path::Path;

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::{ExtractionContext, Lang, LangExtractor, ParsedDocument, ts};

use crate::sources::CBuildContext;
use crate::tsconfig::TsResolution;

#[derive(Debug, Clone, Default)]
pub struct Context {
	pub c: CBuildContext,
	pub ts: TsResolution,
	pub project: Option<String>,
	pub srcset: Option<String>,
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
	extract_with_document(lang, source, path, ctx).0
}

pub fn extract_with_document(
	lang: Lang,
	source: &str,
	path: &Path,
	ctx: &Context,
) -> (CodeGraph, ParsedDocument) {
	let uri = path.to_str().unwrap_or("single-file");
	let anchor = path_anchor(path, ctx);
	let deep = true;
	let document = lang.parse(uri, source);
	let mut graph = match lang {
		Lang::Ts | Lang::Tsx | Lang::Js | Lang::Jsx => {
			let presets = ts::Presets {
				path_aliases: ctx.ts.aliases.clone(),
				sdk_profile: ctx.ts.sdk_profile_for(path).clone(),
				..ts::Presets::default()
			};
			match lang {
				Lang::Ts => {
					extract_parsed::<ts::Lang>(uri, source, &anchor, deep, &presets, &document)
				}
				Lang::Tsx => {
					extract_parsed::<ts::TsxLang>(uri, source, &anchor, deep, &presets, &document)
				}
				Lang::Js => {
					extract_parsed::<ts::JsLang>(uri, source, &anchor, deep, &presets, &document)
				}
				Lang::Jsx => {
					extract_parsed::<ts::JsxLang>(uri, source, &anchor, deep, &presets, &document)
				}
				_ => unreachable!("TypeScript family arm only receives ts/tsx/js/jsx"),
			}
		}
		Lang::Rs => extract_parsed::<code_moniker_core::lang::rs::Lang>(
			uri,
			source,
			&anchor,
			deep,
			&Default::default(),
			&document,
		),
		Lang::Java => extract_parsed::<code_moniker_core::lang::java::Lang>(
			uri,
			source,
			&anchor,
			deep,
			&Default::default(),
			&document,
		),
		Lang::Python => extract_parsed::<code_moniker_core::lang::python::Lang>(
			uri,
			source,
			&anchor,
			deep,
			&Default::default(),
			&document,
		),
		Lang::Go => extract_parsed::<code_moniker_core::lang::go::Lang>(
			uri,
			source,
			&anchor,
			deep,
			&Default::default(),
			&document,
		),
		Lang::C => {
			let presets = ctx.c.extraction_presets();
			extract_parsed::<code_moniker_core::lang::c::Lang>(
				uri, source, &anchor, deep, &presets, &document,
			)
		}
		Lang::Cs => extract_parsed::<code_moniker_core::lang::cs::Lang>(
			uri,
			source,
			&anchor,
			deep,
			&Default::default(),
			&document,
		),
		Lang::Sql => extract_parsed::<code_moniker_core::lang::sql::Lang>(
			uri,
			source,
			&anchor,
			deep,
			&Default::default(),
			&document,
		),
	};
	graph.shrink_to_fit();
	(graph, document)
}

fn extract_parsed<E: LangExtractor>(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	presets: &E::Presets,
	document: &ParsedDocument,
) -> CodeGraph {
	E::extract_parsed(
		ExtractionContext::new(uri, source, anchor, deep, presets),
		document,
	)
}

fn path_anchor(path: &Path, ctx: &Context) -> Moniker {
	let project = ctx.project.as_deref().map(str::as_bytes).unwrap_or(b".");
	let srcset = ctx
		.srcset
		.as_deref()
		.or_else(|| srcset(path))
		.map(str::as_bytes);
	anchor_moniker(project, srcset)
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
	use std::fs;

	use tempfile::tempdir;

	#[test]
	fn path_derived_roots_match_extracted_graph_roots() {
		let cases = [
			(Lang::Rs, "src/tools/mod.rs", ""),
			(Lang::Ts, "src/tools/read.ts", ""),
			(Lang::Tsx, "src/tools/read.tsx", ""),
			(Lang::Js, "src/tools/read.js", ""),
			(Lang::Jsx, "src/tools/read.jsx", ""),
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
	fn typescript_extraction_uses_the_nearest_tsconfig_sdk_profile() {
		let temp = tempdir().expect("tempdir");
		let server = temp.path().join("server");
		let web = temp.path().join("web");
		fs::create_dir_all(&server).expect("server dir");
		fs::create_dir_all(&web).expect("web dir");
		fs::write(
			server.join("tsconfig.json"),
			r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022"]}}"#,
		)
		.expect("server tsconfig");
		fs::write(
			web.join("tsconfig.json"),
			r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022","DOM"]}}"#,
		)
		.expect("web tsconfig");
		let ctx = Context {
			ts: crate::tsconfig::load(temp.path()),
			..Context::default()
		};
		let source = "export function render() { document.body.replaceChildren(); return Promise.resolve(); }";

		let server_graph = extract_with(Lang::Ts, source, &server.join("main.ts"), &ctx);
		let web_graph = extract_with(Lang::Ts, source, &web.join("main.ts"), &ctx);
		let is_sdk_target = |graph: &CodeGraph, name: &[u8]| {
			graph.refs().any(|reference| {
				let segments = reference.target.as_view().segments().collect::<Vec<_>>();
				segments
					.first()
					.is_some_and(|segment| segment.kind == b"sdk")
					&& segments.last().is_some_and(|segment| segment.name == name)
			})
		};

		assert!(is_sdk_target(&web_graph, b"document"));
		assert!(is_sdk_target(&web_graph, b"replaceChildren"));
		assert!(!is_sdk_target(&server_graph, b"document"));
		assert!(!is_sdk_target(&server_graph, b"replaceChildren"));
		assert!(is_sdk_target(&server_graph, b"resolve"));
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
