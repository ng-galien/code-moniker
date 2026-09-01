use std::ops::Range;

use tree_sitter::Tree;

/// Syntax produced by a language SDK before semantic extraction.
///
/// The primary tree represents the source language. Injections hold language
/// regions embedded in opaque host nodes, such as PostgreSQL function bodies.
pub struct ParsedDocument {
	primary: Tree,
	injections: Vec<SyntaxInjection>,
}

impl ParsedDocument {
	pub fn new(primary: Tree) -> Self {
		Self {
			primary,
			injections: Vec::new(),
		}
	}

	pub fn with_injections(primary: Tree, injections: Vec<SyntaxInjection>) -> Self {
		Self {
			primary,
			injections,
		}
	}

	pub fn primary(&self) -> &Tree {
		&self.primary
	}

	pub fn injections(&self) -> &[SyntaxInjection] {
		&self.injections
	}

	pub fn injection_for_host(&self, host_byte_range: Range<usize>) -> Option<&SyntaxInjection> {
		self.injections.iter().find(|injection| {
			injection.host_byte_range.start == host_byte_range.start
				&& injection.host_byte_range.end <= host_byte_range.end
		})
	}

	pub fn injection_within(&self, container_byte_range: Range<usize>) -> Option<&SyntaxInjection> {
		self.injections.iter().find(|injection| {
			injection.host_byte_range.start >= container_byte_range.start
				&& injection.host_byte_range.end <= container_byte_range.end
		})
	}
}

/// Grammar entry point an injected region parses as, reported to syntax consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxEntryPoint {
	/// A complete source file of the injected language.
	Script,
	/// One complete statement of the injected language.
	Statement,
	/// An expression fragment; parsed with the script grammar, errors stay region-local.
	Expression,
	/// A PL/pgSQL block body.
	Block,
}

impl SyntaxEntryPoint {
	pub fn tag(self) -> &'static str {
		match self {
			Self::Script => "script",
			Self::Statement => "statement",
			Self::Expression => "expression",
			Self::Block => "block",
		}
	}
}

pub struct SyntaxInjection {
	language: &'static str,
	entry_point: SyntaxEntryPoint,
	host_byte_range: Range<usize>,
	content_byte_range: Range<usize>,
	tree: Tree,
	/// Source the tree was actually parsed from, with the byte length of its synthetic prefix,
	/// when it differs from the document content (PostgreSQL parses PL/pgSQL expressions as
	/// `SELECT expr`, exactly like plpgsql's own read_sql_expression). The rendered region is
	/// the tree node spanning the content, never the wrapper.
	analysis: Option<(String, usize)>,
	/// Injections inside this injection's content, in document byte coordinates.
	nested: Vec<SyntaxInjection>,
}

impl SyntaxInjection {
	pub fn new(
		language: &'static str,
		entry_point: SyntaxEntryPoint,
		host_byte_range: Range<usize>,
		content_byte_range: Range<usize>,
		tree: Tree,
	) -> Self {
		Self {
			language,
			entry_point,
			host_byte_range,
			content_byte_range,
			tree,
			analysis: None,
			nested: Vec::new(),
		}
	}

	pub fn with_nested(mut self, nested: Vec<SyntaxInjection>) -> Self {
		self.nested = nested;
		self
	}

	pub fn with_analysis(mut self, source: String, prefix: usize) -> Self {
		self.analysis = Some((source, prefix));
		self
	}

	/// Byte length of the synthetic prefix in front of the parsed content, 0 without one.
	pub fn analysis_prefix(&self) -> usize {
		self.analysis.as_ref().map_or(0, |(_, prefix)| *prefix)
	}

	/// The source the tree byte offsets refer to, when it is not the document content itself.
	pub fn analysis_source(&self) -> Option<&str> {
		self.analysis.as_ref().map(|(source, _)| source.as_str())
	}

	/// The node the region renders from: the smallest named node spanning exactly the content
	/// within the analysis source. Without a synthetic prefix that is the tree root.
	pub fn render_root(&self) -> tree_sitter::Node<'_> {
		let root = self.tree.root_node();
		let prefix = self.analysis_prefix();
		if prefix == 0 {
			return root;
		}
		let content = prefix + (self.content_byte_range.end - self.content_byte_range.start);
		covering_node(root, &(prefix..content)).unwrap_or(root)
	}

	pub fn language(&self) -> &'static str {
		self.language
	}

	pub fn entry_point(&self) -> SyntaxEntryPoint {
		self.entry_point
	}

	pub fn nested(&self) -> &[SyntaxInjection] {
		&self.nested
	}

	/// The nested injection a host node stands in for. An expression injection's range is the
	/// node's content with trailing whitespace trimmed, so the match is on start and containment.
	pub fn nested_for_host(&self, host_byte_range: Range<usize>) -> Option<&SyntaxInjection> {
		self.nested.iter().find(|injection| {
			injection.host_byte_range.start == host_byte_range.start
				&& injection.host_byte_range.end <= host_byte_range.end
		})
	}

	pub fn host_byte_range(&self) -> Range<usize> {
		self.host_byte_range.clone()
	}

	pub fn content_byte_range(&self) -> Range<usize> {
		self.content_byte_range.clone()
	}

	pub fn tree(&self) -> &Tree {
		&self.tree
	}
}

/// The shallowest named node spanning exactly `range`, when the parse isolated one. Shallowest,
/// so a lone identifier still renders under its classifying node (`columnref`), never bare.
pub fn covering_node<'tree>(
	node: tree_sitter::Node<'tree>,
	range: &Range<usize>,
) -> Option<tree_sitter::Node<'tree>> {
	if node.start_byte() > range.start || node.end_byte() < range.end {
		return None;
	}
	if node.is_named() && node.start_byte() == range.start && node.end_byte() == range.end {
		return Some(node);
	}
	let mut cursor = node.walk();
	let children: Vec<tree_sitter::Node<'tree>> = node.named_children(&mut cursor).collect();
	for child in children {
		if let Some(found) = covering_node(child, range) {
			return Some(found);
		}
	}
	None
}
