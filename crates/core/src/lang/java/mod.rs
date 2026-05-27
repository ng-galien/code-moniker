use tree_sitter::{Language, Parser, Tree};

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;

use crate::lang::KindSpec;

mod kinds;
pub mod pom_manifest;
mod sdk_pipeline;

#[derive(Clone, Debug, Default)]
pub struct Presets {
	pub external_packages: Vec<String>,
}

pub fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	let language: Language = tree_sitter_java::LANGUAGE.into();
	parser
		.set_language(&language)
		.expect("failed to load tree-sitter Java grammar");
	parser
		.parse(source, None)
		.expect("tree-sitter parse returned None on a non-cancelled call")
}

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
	"class",
	"interface",
	"enum",
	"record",
	"annotation_type",
	"method",
	"constructor",
	"field",
	"enum_constant",
];

const DEF_KIND_SPECS: &[KindSpec] = &[
	KindSpec::new("class", Shape::Type, 20, "class"),
	KindSpec::new("interface", Shape::Type, 21, "interface"),
	KindSpec::new("enum", Shape::Type, 22, "enum"),
	KindSpec::new("record", Shape::Type, 23, "record"),
	KindSpec::new("annotation_type", Shape::Type, 24, "annotation_type"),
	KindSpec::new("enum_constant", Shape::Value, 30, "enum_constant"),
	KindSpec::new("field", Shape::Value, 31, "field"),
	KindSpec::new("constructor", Shape::Callable, 40, "constructor"),
	KindSpec::new("method", Shape::Callable, 41, "method"),
];

impl crate::lang::LangExtractor for Lang {
	type Presets = Presets;
	const LANG_TAG: &'static str = "java";
	const ALLOWED_KINDS: &'static [&'static str] = DEF_KINDS;
	const KIND_SPECS: &'static [KindSpec] = DEF_KIND_SPECS;
	const ALLOWED_VISIBILITIES: &'static [&'static str] =
		&["public", "protected", "package", "private"];

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
