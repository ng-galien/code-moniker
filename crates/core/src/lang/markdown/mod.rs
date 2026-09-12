use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;
use crate::lang::{ExtractionContext, KindSpec, LangExtractor, ParsedDocument, structured};

pub struct Lang;

impl LangExtractor for Lang {
	type Presets = ();
	const LANG_TAG: &'static str = "markdown";
	const ALLOWED_KINDS: &'static [&'static str] = &["section", "code_block", "link_definition"];
	const KIND_SPECS: &'static [KindSpec] = &[
		KindSpec::new("section", Shape::Namespace, 10, "section"),
		KindSpec::new("code_block", Shape::Value, 20, "code block"),
		KindSpec::new("link_definition", Shape::Value, 30, "link definition"),
	];
	const ALLOWED_VISIBILITIES: &'static [&'static str] = &[];

	fn file_root(uri: &str, anchor: &Moniker) -> Option<Moniker> {
		Some(structured::file_root(uri, anchor, b"markdown"))
	}

	fn parse(_uri: &str, source: &str) -> ParsedDocument {
		structured::parse(source, tree_sitter_md::LANGUAGE)
	}

	fn extract_parsed(context: ExtractionContext<'_, ()>, document: &ParsedDocument) -> CodeGraph {
		let mut defs = structured::Definitions::new(structured::file_root(
			context.uri,
			context.anchor,
			b"markdown",
		));
		let events = document_events(document);
		let ends = section_ends(&events, context.source.len());
		let mut sections: Vec<(usize, Moniker)> = Vec::new();
		for (index, node) in events.into_iter().enumerate() {
			if let Some(level) = heading_level(node) {
				while sections
					.last()
					.is_some_and(|(previous, _)| *previous >= level)
				{
					sections.pop();
				}
				let parent = sections
					.last()
					.map(|(_, owner)| owner)
					.unwrap_or(&defs.root)
					.clone();
				let name = node
					.child_by_field_name("heading_content")
					.map(|content| structured::text(content, context.source).trim())
					.unwrap_or("");
				let owner = defs.add_range(
					&parent,
					b"section",
					name,
					(node.start_byte() as u32, ends[index] as u32),
				);
				sections.push((level, owner));
			} else {
				let parent = sections
					.last()
					.map(|(_, owner)| owner)
					.unwrap_or(&defs.root)
					.clone();
				let children = structured::children(node);
				if node.kind() == "link_reference_definition" {
					if let Some(label) = children.iter().find(|child| child.kind() == "link_label")
					{
						defs.add(
							&parent,
							b"link_definition",
							structured::text(*label, context.source),
							node,
						);
					}
				} else {
					let name = children
						.iter()
						.find(|child| child.kind() == "info_string")
						.map(|info| structured::text(*info, context.source).trim())
						.unwrap_or("code");
					defs.add(&parent, b"code_block", name, node);
				}
			}
		}
		defs.finish()
	}
}

fn heading_level(node: tree_sitter::Node<'_>) -> Option<usize> {
	match node.kind() {
		"atx_heading" => structured::children(node)
			.iter()
			.find_map(|child| match child.kind() {
				"atx_h1_marker" => Some(1),
				"atx_h2_marker" => Some(2),
				"atx_h3_marker" => Some(3),
				"atx_h4_marker" => Some(4),
				"atx_h5_marker" => Some(5),
				"atx_h6_marker" => Some(6),
				_ => None,
			}),
		"setext_heading" => Some(
			if structured::children(node)
				.iter()
				.any(|child| child.kind() == "setext_h1_underline")
			{
				1
			} else {
				2
			},
		),
		_ => None,
	}
}

fn document_events(document: &ParsedDocument) -> Vec<tree_sitter::Node<'_>> {
	let mut events = Vec::new();
	let mut pending = vec![document.primary().root_node()];
	while let Some(node) = pending.pop() {
		match node.kind() {
			"atx_heading"
			| "setext_heading"
			| "fenced_code_block"
			| "indented_code_block"
			| "link_reference_definition" => events.push(node),
			_ => pending.extend(structured::children(node).into_iter().rev()),
		}
	}
	events
}

fn section_ends(events: &[tree_sitter::Node<'_>], source_end: usize) -> Vec<usize> {
	let mut ends = vec![source_end; events.len()];
	let mut stack: Vec<(usize, usize)> = Vec::new();
	for (index, node) in events.iter().enumerate() {
		if let Some(level) = heading_level(*node) {
			while stack.last().is_some_and(|(_, previous)| *previous >= level) {
				let (previous, _) = stack.pop().expect("stack is nonempty");
				ends[previous] = node.start_byte();
			}
			stack.push((index, level));
		}
	}
	ends
}
