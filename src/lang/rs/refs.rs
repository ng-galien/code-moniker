//! Refs extraction: `use` declarations → `imports_symbol`,
//! `impl Trait for Type` → `implements`.
//!
//! Use-target monikers are emitted under the importer's project
//! authority as path-only segments (`std::collections::HashMap` →
//! `<project>/path:std/path:collections/path:HashMap`). Without a
//! `presets` parameter the extractor cannot know whether a path is
//! external (std, a crate dep) or project-local — same legacy shape
//! TS uses for bare imports today.

use tree_sitter::Node;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{impl_type_name, node_position};
use super::kinds;
use super::walker::Walker;

impl<'src> Walker<'src> {
	pub(super) fn handle_use(
		&self,
		node: Node<'_>,
		parent: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(arg) = node.child_by_field_name("argument") else { return };
		let pos = node_position(node);
		let mut leaves: Vec<Vec<String>> = Vec::new();
		collect_use_leaves(arg, self.source_bytes, &mut Vec::new(), &mut leaves);
		for path in leaves {
			let target = build_use_target(&self.module, &path);
			let _ = graph.add_ref(parent, target, kinds::IMPORTS_SYMBOL, Some(pos));
		}
	}

	pub(super) fn handle_impl_trait_for(
		&self,
		impl_node: Node<'_>,
		type_moniker: &Moniker,
		graph: &mut CodeGraph,
	) {
		let Some(trait_node) = impl_node.child_by_field_name("trait") else { return };
		let Some(trait_name) = impl_type_name(trait_node, self.source_bytes) else { return };
		let trait_moniker = MonikerBuilder::from_view(self.module.as_view())
			.segment(kinds::INTERFACE, trait_name.as_bytes())
			.build();
		let _ = graph.add_ref(
			type_moniker,
			trait_moniker,
			kinds::IMPLEMENTS,
			Some(node_position(impl_node)),
		);
	}
}

/// Walk the `argument` of a `use_declaration` and collect every leaf
/// import path (one per imported symbol). `path_prefix` is the chain
/// of identifiers on the way down; each call appends and recurses into
/// list/scoped variants.
fn collect_use_leaves(
	node: Node<'_>,
	source: &[u8],
	path_prefix: &mut Vec<String>,
	out: &mut Vec<Vec<String>>,
) {
	match node.kind() {
		"identifier" | "crate" | "self" | "super" => {
			if let Ok(s) = node.utf8_text(source) {
				let mut leaf = path_prefix.clone();
				leaf.push(s.to_string());
				out.push(leaf);
			}
		}
		"scoped_identifier" => {
			let mut prefix = path_prefix.clone();
			collect_scoped_path(node, source, &mut prefix);
			if !prefix.is_empty() {
				out.push(prefix);
			}
		}
		"scoped_use_list" => {
			let mut prefix = path_prefix.clone();
			if let Some(path) = node.child_by_field_name("path") {
				collect_scoped_path_into(path, source, &mut prefix);
			}
			if let Some(list) = node.child_by_field_name("list") {
				let mut cursor = list.walk();
				for child in list.named_children(&mut cursor) {
					collect_use_leaves(child, source, &mut prefix.clone(), out);
				}
			}
		}
		"use_list" => {
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				collect_use_leaves(child, source, &mut path_prefix.clone(), out);
			}
		}
		"use_as_clause" => {
			// Alias is dropped: DefRecord has nowhere to store it. The
			// imported name remains the source target.
			if let Some(path) = node.child_by_field_name("path") {
				collect_use_leaves(path, source, path_prefix, out);
			}
		}
		"use_wildcard" => {
			// `a::b::*` — emit the parent path itself as a name-only
			// imports_symbol; the `*` is information we'd need DefRecord
			// metadata to preserve. Recurse on the child so a
			// scoped_identifier (`a::b`) splits into multiple segments
			// instead of being captured as one literal `a::b` string.
			let mut leaf = path_prefix.clone();
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				collect_scoped_path_into(child, source, &mut leaf);
			}
			if !leaf.is_empty() {
				out.push(leaf);
			}
		}
		_ => {}
	}
}

/// Linearize a `scoped_identifier` (`a::b::c`) into a flat `Vec<String>`.
fn collect_scoped_path(node: Node<'_>, source: &[u8], out: &mut Vec<String>) {
	collect_scoped_path_into(node, source, out);
}

fn collect_scoped_path_into(node: Node<'_>, source: &[u8], out: &mut Vec<String>) {
	if node.kind() == "scoped_identifier" {
		if let Some(path) = node.child_by_field_name("path") {
			collect_scoped_path_into(path, source, out);
		}
		if let Some(name) = node.child_by_field_name("name") {
			if let Ok(s) = name.utf8_text(source) {
				out.push(s.to_string());
			}
		}
		return;
	}
	if let Ok(s) = node.utf8_text(source) {
		out.push(s.to_string());
	}
}

fn build_use_target(module: &Moniker, path: &[String]) -> Moniker {
	// Target is rooted at the importer's project authority — the
	// extractor cannot resolve external vs project-local without
	// presets. Same legacy compromise as TS bare imports.
	let mut b = MonikerBuilder::new();
	b.project(module.as_view().project());
	for piece in path {
		b.segment(kinds::PATH, piece.as_bytes());
	}
	b.build()
}
