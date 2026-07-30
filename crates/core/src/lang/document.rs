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
		self.injections
			.iter()
			.find(|injection| injection.host_byte_range == host_byte_range)
	}

	pub fn injection_within(&self, container_byte_range: Range<usize>) -> Option<&SyntaxInjection> {
		self.injections.iter().find(|injection| {
			injection.host_byte_range.start >= container_byte_range.start
				&& injection.host_byte_range.end <= container_byte_range.end
		})
	}
}

pub struct SyntaxInjection {
	language: &'static str,
	host_byte_range: Range<usize>,
	content_byte_range: Range<usize>,
	tree: Tree,
}

impl SyntaxInjection {
	pub fn new(
		language: &'static str,
		host_byte_range: Range<usize>,
		content_byte_range: Range<usize>,
		tree: Tree,
	) -> Self {
		Self {
			language,
			host_byte_range,
			content_byte_range,
			tree,
		}
	}

	pub fn language(&self) -> &'static str {
		self.language
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
