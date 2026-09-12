use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;
use crate::lang::{ExtractionContext, KindSpec, LangExtractor, ParsedDocument, structured};

pub struct Lang;

impl LangExtractor for Lang {
	type Presets = ();
	const LANG_TAG: &'static str = "yaml";
	const ALLOWED_KINDS: &'static [&'static str] = &["document", "key", "item"];
	const KIND_SPECS: &'static [KindSpec] = &[
		KindSpec::new("document", Shape::Namespace, 10, "document"),
		KindSpec::new("key", Shape::Namespace, 20, "key"),
		KindSpec::new("item", Shape::Namespace, 30, "sequence item"),
	];
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &[];

	fn file_root(uri: &str, anchor: &Moniker) -> Option<Moniker> {
		Some(structured::file_root(uri, anchor, b"yaml"))
	}

	fn parse(_uri: &str, source: &str) -> ParsedDocument {
		structured::parse(source, tree_sitter_yaml::LANGUAGE)
	}

	fn extract_parsed(context: ExtractionContext<'_, ()>, document: &ParsedDocument) -> CodeGraph {
		let mut defs = structured::Definitions::new(structured::file_root(
			context.uri,
			context.anchor,
			b"yaml",
		));
		let mut pending = vec![(document.primary().root_node(), defs.root.clone())];
		let mut document_index = 0;
		while let Some((node, parent)) = pending.pop() {
			match node.kind() {
				"document" => {
					let owner = defs.add(&parent, b"document", &document_index.to_string(), node);
					document_index += 1;
					pending.extend(
						structured::children(node)
							.into_iter()
							.rev()
							.map(|child| (child, owner.clone())),
					);
				}
				"flow_node" if node.parent().is_some_and(|p| p.kind() == "flow_mapping") => {
					defs.add(
						&parent,
						b"key",
						structured::text(node, context.source),
						node,
					);
				}
				"block_mapping_pair" | "flow_pair" => {
					// YAML keys can be arbitrary nodes, including tagged collections.
					// Retain their source spelling; never coerce or evaluate them.
					let name = node
						.child_by_field_name("key")
						.map(|key| structured::text(key, context.source))
						.unwrap_or("");
					let owner = defs.add(&parent, b"key", name, node);
					if let Some(value) = node.child_by_field_name("value") {
						pending.push((value, owner));
					}
				}
				"block_sequence" | "flow_sequence" => {
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
