//! Shared source identities and SDK emission for document/data extractors.
use std::borrow::Cow;

use rustc_hash::FxHashMap;
use tree_sitter::{Node, Parser};

use crate::core::code_graph::{CodeGraph, Position};
use crate::core::moniker::{Moniker, MonikerBuilder};
use crate::lang::ParsedDocument;
use crate::lang::sdk::{
	DiscoveredDef, DiscoveredFile, GraphEmitter, ImportTable, Namespace, ScopeTree,
};
use crate::lang::tree_util::node_position;

pub(super) fn parse(source: &str, language: tree_sitter_language::LanguageFn) -> ParsedDocument {
	let mut parser = Parser::new();
	let is_markdown =
		std::ptr::fn_addr_eq(language.into_raw(), tree_sitter_md::LANGUAGE.into_raw());
	let language: tree_sitter::Language = language.into();
	parser
		.set_language(&language)
		.expect("document grammar must load");
	let parser_source = if is_markdown {
		markdown_parser_source(source)
	} else {
		Cow::Borrowed(source)
	};
	ParsedDocument::new(
		parser
			.parse(parser_source.as_ref(), None)
			.expect("document parser must return a tree"),
	)
}

/// tree-sitter-markdown 0.5.3 passes Unicode lookahead to C's byte-limited
/// `isdigit`. A digit before such a code point cannot form a list marker, so a
/// parser-only ASCII substitution preserves structure and byte offsets while
/// definitions continue to slice their names from the original source.
fn markdown_parser_source(source: &str) -> Cow<'_, str> {
	let mut sanitized = None;
	for (index, character) in source.char_indices() {
		if character > '\u{ff}' && index > 0 && source.as_bytes()[index - 1].is_ascii_digit() {
			sanitized.get_or_insert_with(|| source.as_bytes().to_vec())[index - 1] = b'x';
		}
	}
	match sanitized {
		Some(bytes) => {
			Cow::Owned(String::from_utf8(bytes).expect("ASCII substitution keeps UTF-8"))
		}
		None => Cow::Borrowed(source),
	}
}

pub(super) fn file_root(uri: &str, anchor: &Moniker, language: &[u8]) -> Moniker {
	let mut builder = MonikerBuilder::from_view(anchor.as_view());
	builder.segment(b"lang", language);
	let normalized = uri.replace('\\', "/");
	let parts: Vec<_> = normalized
		.split('/')
		.filter(|part| !part.is_empty())
		.collect();
	if let Some((file, directories)) = parts.split_last() {
		for directory in directories {
			builder.segment(b"dir", directory.as_bytes());
		}
		builder.segment(b"module", file.as_bytes());
	} else {
		builder.segment(b"module", b"");
	}
	builder.build()
}

pub(super) fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
	&source[node.byte_range()]
}

pub(super) fn children(node: Node<'_>) -> Vec<Node<'_>> {
	node.named_children(&mut node.walk()).collect()
}

pub(super) struct Definitions {
	pub root: Moniker,
	defs: Vec<DiscoveredDef>,
	occurrences: FxHashMap<Moniker, usize>,
}

impl Definitions {
	pub fn new(root: Moniker) -> Self {
		Self {
			root,
			defs: Vec::new(),
			occurrences: FxHashMap::default(),
		}
	}

	pub fn add(
		&mut self,
		owner: &Moniker,
		definition_kind: &'static [u8],
		label: &str,
		node: Node<'_>,
	) -> Moniker {
		self.add_range(owner, definition_kind, label, node_position(node))
	}

	pub fn add_range(
		&mut self,
		parent: &Moniker,
		kind: &'static [u8],
		name: &str,
		position: Position,
	) -> Moniker {
		let escaped = escape_occurrence_suffix(name);
		let moniker = child(parent, kind, &escaped);
		let occurrence = self.occurrences.entry(moniker.clone()).or_default();
		*occurrence += 1;
		let moniker = if *occurrence == 1 {
			moniker
		} else {
			child(parent, kind, &format!("{escaped}~{occurrence}"))
		};
		self.defs.push(DiscoveredDef {
			moniker: moniker.clone(),
			parent: parent.clone(),
			namespace: Namespace::Unified,
			name: name.as_bytes().to_vec(),
			kind,
			visibility: b"",
			signature: Vec::new(),
			position: Some(position),
			call_name: Vec::new(),
			call_arity: None,
		});
		moniker
	}

	pub fn finish(self) -> CodeGraph {
		let discovered = DiscoveredFile::new(
			self.root.clone(),
			b"module",
			self.defs,
			ScopeTree::new(self.root),
			ImportTable::default(),
		);
		GraphEmitter::emit(&discovered, &[])
			.expect("document definitions must form a unique anchored tree")
	}
}

/// Escapes the occurrence delimiter so a literal `name~2` remains distinct
/// from the second occurrence of `name`.
fn escape_occurrence_suffix(name: &str) -> String {
	name.replace('~', "~0")
}

fn child(parent: &Moniker, kind: &[u8], name: &str) -> Moniker {
	let mut builder = MonikerBuilder::from_view(parent.as_view());
	builder.segment(kind, name.as_bytes());
	builder.build()
}
