//! Shared source identities and SDK emission for document/data extractors.
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
	parser
		.set_language(&language.into())
		.expect("document grammar must load");
	ParsedDocument::new(
		parser
			.parse(source, None)
			.expect("document parser must return a tree"),
	)
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
		parent: &Moniker,
		kind: &'static [u8],
		name: &str,
		node: Node<'_>,
	) -> Moniker {
		self.add_range(parent, kind, name, node_position(node))
	}

	pub fn add_range(
		&mut self,
		parent: &Moniker,
		kind: &'static [u8],
		name: &str,
		position: Position,
	) -> Moniker {
		// Escape the suffix delimiter before adding an occurrence suffix. Thus a
		// literal `name~2` cannot collide with the second occurrence of `name`.
		let escaped = name.replace('~', "~0");
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

fn child(parent: &Moniker, kind: &[u8], name: &str) -> Moniker {
	let mut builder = MonikerBuilder::from_view(parent.as_view());
	builder.segment(kind, name.as_bytes());
	builder.build()
}
