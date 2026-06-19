use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;

use crate::lang::KindSpec;

pub mod cargo_manifest;
mod kinds;
mod sdk_pipeline;

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_rust::LANGUAGE.into();
	parser.set_language(&language).unwrap_or_else(|err| {
		panic!("failed to load tree-sitter Rust grammar: {err}");
	});
	parser.parse(source, None).unwrap_or_else(|| {
		panic!("tree-sitter parse returned None on a non-cancelled call");
	})
}

#[derive(Clone, Debug, Default)]
pub struct Presets {}

pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	extract_sdk(uri, source, anchor, deep, presets)
}

pub fn extract_sdk(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	presets: &Presets,
) -> CodeGraph {
	sdk_pipeline::extract(uri, source, anchor, deep, presets)
}

pub struct Lang;

const DEF_KINDS: &[&str] = &[
	"struct",
	"enum",
	"enum_constant",
	"trait",
	"impl",
	"fn",
	"macro",
	"method",
	"test",
	"const",
	"static",
	"path",
	"type",
];

const DEF_KIND_SPECS: &[KindSpec] = &[
	KindSpec::new("impl", Shape::Namespace, 10, "impl"),
	KindSpec::new("struct", Shape::Type, 20, "struct"),
	KindSpec::new("enum", Shape::Type, 21, "enum"),
	KindSpec::new("trait", Shape::Type, 22, "trait"),
	KindSpec::new("type", Shape::Type, 23, "type"),
	KindSpec::new("fn", Shape::Callable, 40, "fn"),
	KindSpec::new("macro", Shape::Callable, 41, "macro"),
	KindSpec::new("method", Shape::Callable, 42, "method"),
	KindSpec::new("test", Shape::Callable, 43, "test"),
	KindSpec::new("enum_constant", Shape::Value, 60, "enum_constant"),
	KindSpec::new("const", Shape::Value, 61, "const"),
	KindSpec::new("static", Shape::Value, 62, "static"),
	KindSpec::new("path", Shape::Value, 63, "path"),
];

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "rs";
	const ALLOWED_KINDS: &'static [&'static str] = DEF_KINDS;
	const KIND_SPECS: &'static [KindSpec] = DEF_KIND_SPECS;
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &["public", "private", "module"];

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
