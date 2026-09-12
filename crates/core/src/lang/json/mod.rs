use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;
use crate::lang::{ExtractionContext, KindSpec, LangExtractor, ParsedDocument, structured};

pub struct Lang;

impl LangExtractor for Lang {
	type Presets = ();
	const LANG_TAG: &'static str = "json";
	const ALLOWED_KINDS: &'static [&'static str] = &["key", "item"];
	const KIND_SPECS: &'static [KindSpec] = &[
		KindSpec::new("key", Shape::Namespace, 10, "key"),
		KindSpec::new("item", Shape::Namespace, 20, "array item"),
	];
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &[];

	fn file_root(uri: &str, anchor: &Moniker) -> Option<Moniker> {
		Some(structured::file_root(uri, anchor, b"json"))
	}

	fn parse(_uri: &str, source: &str) -> ParsedDocument {
		structured::parse(source, tree_sitter_json::LANGUAGE)
	}

	fn extract_parsed(context: ExtractionContext<'_, ()>, document: &ParsedDocument) -> CodeGraph {
		let mut defs = structured::Definitions::new(structured::file_root(
			context.uri,
			context.anchor,
			b"json",
		));
		let mut pending = vec![(document.primary().root_node(), defs.root.clone())];
		while let Some((node, parent)) = pending.pop() {
			match node.kind() {
				"pair" => {
					if let Some(key) = node.child_by_field_name("key") {
						let raw = structured::text(key, context.source);
						let name =
							serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_string());
						let owner = defs.add(&parent, b"key", &name, node);
						if let Some(value) = node.child_by_field_name("value") {
							pending.push((value, owner));
						}
					}
				}
				"array" => {
					let mut items = Vec::new();
					for (index, item) in structured::children(node)
						.into_iter()
						.filter(|n| !n.is_extra())
						.enumerate()
					{
						let owner = defs.add(&parent, b"item", &index.to_string(), item);
						items.push((item, owner));
					}
					pending.extend(items.into_iter().rev());
				}
				_ => pending.extend(
					structured::children(node)
						.into_iter()
						.rev()
						.map(|child| (child, parent.clone())),
				),
			}
		}
		defs.finish()
	}
}
