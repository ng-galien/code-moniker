use std::path::Path;

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::{Lang, ts};

use crate::tsconfig::TsResolution;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum RustExtractionPipeline {
	Legacy,
	#[default]
	Sdk,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum JavaExtractionPipeline {
	Legacy,
	#[default]
	Sdk,
}

#[derive(Debug, Clone, Default)]
pub struct Context {
	pub ts: TsResolution,
	pub project: Option<String>,
	pub rust_pipeline: RustExtractionPipeline,
	pub java_pipeline: JavaExtractionPipeline,
}

pub fn extract(lang: Lang, source: &str, path: &Path) -> CodeGraph {
	extract_with(lang, source, path, &Context::default())
}

pub fn extract_with(lang: Lang, source: &str, path: &Path, ctx: &Context) -> CodeGraph {
	let uri = path.to_str().unwrap_or("single-file");
	let project = ctx.project.as_deref().map(str::as_bytes).unwrap_or(b".");
	let anchor = anchor_moniker(project, srcset(path).map(str::as_bytes));
	let deep = true;
	match lang {
		Lang::Ts => {
			let presets = ts::Presets {
				path_aliases: ctx.ts.aliases.clone(),
				..ts::Presets::default()
			};
			ts::extract(uri, source, &anchor, deep, &presets)
		}
		Lang::Rs => match ctx.rust_pipeline {
			RustExtractionPipeline::Legacy => {
				panic!("Rust legacy extraction pipeline was removed; use the SDK pipeline")
			}
			RustExtractionPipeline::Sdk => code_moniker_core::lang::rs::extract_sdk(
				uri,
				source,
				&anchor,
				deep,
				&code_moniker_core::lang::rs::Presets::default(),
			),
		},
		Lang::Java => match ctx.java_pipeline {
			JavaExtractionPipeline::Legacy => {
				panic!("Java legacy extraction pipeline was removed; use the SDK pipeline")
			}
			JavaExtractionPipeline::Sdk => code_moniker_core::lang::java::extract_sdk(
				uri,
				source,
				&anchor,
				deep,
				&code_moniker_core::lang::java::Presets::default(),
			),
		},
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
	fn rust_and_java_default_to_sdk_pipeline() {
		assert_eq!(
			RustExtractionPipeline::default(),
			RustExtractionPipeline::Sdk
		);
		assert_eq!(
			JavaExtractionPipeline::default(),
			JavaExtractionPipeline::Sdk
		);
	}

	#[test]
	#[should_panic(expected = "Rust legacy extraction pipeline was removed")]
	fn rust_legacy_pipeline_panics() {
		let ctx = Context {
			rust_pipeline: RustExtractionPipeline::Legacy,
			..Context::default()
		};
		let _ = extract_with(Lang::Rs, "fn main() {}", Path::new("main.rs"), &ctx);
	}

	#[test]
	#[should_panic(expected = "Java legacy extraction pipeline was removed")]
	fn java_legacy_pipeline_panics() {
		let ctx = Context {
			java_pipeline: JavaExtractionPipeline::Legacy,
			..Context::default()
		};
		let _ = extract_with(Lang::Java, "class Main {}", Path::new("Main.java"), &ctx);
	}
}
