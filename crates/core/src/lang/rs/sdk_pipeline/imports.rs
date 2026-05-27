use tree_sitter::Node;

use crate::lang::sdk::ImportTree;
use crate::lang::tree_util::node_slice;

use super::syntax::{named_children, path_pieces};

pub(super) fn import_tree(node: Node<'_>, source: &[u8]) -> Option<ImportTree> {
	match node.kind() {
		"identifier" => named_import_tree(node_slice(node, source)),
		"crate" | "super" => Some(ImportTree::Name(node_slice(node, source).to_vec())),
		"self" => Some(ImportTree::SelfImport),
		"scoped_identifier" | "scoped_type_identifier" => path_tree(path_pieces(node, source)),
		"scoped_use_list" => scoped_use_list_tree(node, source),
		"use_list" => use_list_tree(node, source),
		"use_as_clause" => alias_tree(node, source),
		"use_wildcard" => wildcard_tree(node, source),
		_ => None,
	}
}

fn named_import_tree(name: &[u8]) -> Option<ImportTree> {
	if name == b"self" {
		return Some(ImportTree::SelfImport);
	}
	Some(ImportTree::Name(name.to_vec()))
}

fn scoped_use_list_tree(node: Node<'_>, source: &[u8]) -> Option<ImportTree> {
	let prefix = node
		.child_by_field_name("path")
		.map(|path| path_pieces(path, source))?;
	let list = node.child_by_field_name("list")?;
	Some(ImportTree::Path {
		prefix,
		tree: Box::new(import_tree(list, source)?),
	})
}

fn use_list_tree(node: Node<'_>, source: &[u8]) -> Option<ImportTree> {
	let items = named_children(node)
		.filter_map(|child| import_tree(child, source))
		.collect::<Vec<_>>();
	Some(ImportTree::Group(items))
}

fn alias_tree(node: Node<'_>, source: &[u8]) -> Option<ImportTree> {
	let path = node.child_by_field_name("path")?;
	let alias = node.child_by_field_name("alias")?;
	let mut segments = path_pieces(path, source);
	let name = segments.pop()?;
	let alias = node_slice(alias, source).to_vec();
	if segments.is_empty() {
		return Some(ImportTree::Alias { name, alias });
	}
	Some(ImportTree::Path {
		prefix: segments,
		tree: Box::new(ImportTree::Alias { name, alias }),
	})
}

fn wildcard_tree(node: Node<'_>, source: &[u8]) -> Option<ImportTree> {
	let prefix = named_children(node)
		.flat_map(|child| path_pieces(child, source))
		.collect::<Vec<_>>();
	if prefix.is_empty() {
		return Some(ImportTree::Wildcard);
	}
	Some(ImportTree::Path {
		prefix,
		tree: Box::new(ImportTree::Wildcard),
	})
}

fn path_tree(mut segments: Vec<Vec<u8>>) -> Option<ImportTree> {
	let name = segments.pop()?;
	if segments.is_empty() {
		return Some(ImportTree::Name(name));
	}
	Some(ImportTree::Path {
		prefix: segments,
		tree: Box::new(ImportTree::Name(name)),
	})
}
