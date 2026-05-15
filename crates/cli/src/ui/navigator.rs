use std::collections::BTreeSet;

use code_moniker_core::core::code_graph::DefRecord;

use crate::inspect::{DefLocation, SessionIndex};

use super::filter::NavFilter;
use super::{def_kind, last_name};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NavNodeKind {
	Root,
	Lang,
	Dir,
	File(usize),
	Def(DefLocation),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NavNode {
	pub(super) key: String,
	pub(super) label: String,
	pub(super) kind: NavNodeKind,
	pub(super) children: Vec<NavNode>,
	pub(super) file_count: usize,
	pub(super) def_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NavRow {
	pub(super) key: String,
	pub(super) label: String,
	pub(super) kind: NavNodeKind,
	pub(super) depth: usize,
	pub(super) has_children: bool,
	pub(super) file_count: usize,
	pub(super) def_count: usize,
}

impl NavNode {
	fn new(key: String, label: String, kind: NavNodeKind) -> Self {
		Self {
			key,
			label,
			kind,
			children: Vec::new(),
			file_count: 0,
			def_count: 0,
		}
	}
}

pub(super) fn build_navigator(index: &SessionIndex) -> NavNode {
	let mut root = NavNode::new("root".to_string(), "root".to_string(), NavNodeKind::Root);
	for (file_idx, file) in index.files.iter().enumerate() {
		let lang_key = format!("lang:{}", file.lang.tag());
		let lang = child_mut(
			&mut root,
			lang_key,
			file.lang.tag().to_string(),
			NavNodeKind::Lang,
		);
		let mut parent = lang;
		let path_parts: Vec<String> = file
			.rel_path
			.parent()
			.into_iter()
			.flat_map(|path| path.components())
			.filter_map(|component| component.as_os_str().to_str())
			.map(ToOwned::to_owned)
			.collect();
		let mut path_key = String::new();
		for part in path_parts {
			if !path_key.is_empty() {
				path_key.push('/');
			}
			path_key.push_str(&part);
			parent = child_mut(
				parent,
				format!("dir:{}:{path_key}", file.lang.tag()),
				part,
				NavNodeKind::Dir,
			);
		}
		let file_label = file
			.rel_path
			.file_name()
			.and_then(|name| name.to_str())
			.unwrap_or_else(|| file.rel_path.to_str().unwrap_or("<file>"))
			.to_string();
		let mut file_node = NavNode::new(
			format!("file:{file_idx}"),
			file_label,
			NavNodeKind::File(file_idx),
		);
		file_node.children = symbol_children(index, file_idx, None);
		parent.children.push(file_node);
	}
	sort_nav(&mut root);
	compute_nav_counts(&mut root);
	root
}

fn child_mut(node: &mut NavNode, key: String, label: String, kind: NavNodeKind) -> &mut NavNode {
	let idx = match node.children.iter().position(|child| child.key == key) {
		Some(idx) => idx,
		None => {
			node.children.push(NavNode::new(key, label, kind));
			node.children.len() - 1
		}
	};
	&mut node.children[idx]
}

fn sort_nav(node: &mut NavNode) {
	node.children
		.sort_by(|a, b| nav_sort_key(a).cmp(&nav_sort_key(b)));
	for child in &mut node.children {
		sort_nav(child);
	}
}

fn nav_sort_key(node: &NavNode) -> (u8, &str) {
	let group = match node.kind {
		NavNodeKind::Root => 0,
		NavNodeKind::Lang => 1,
		NavNodeKind::Dir => 2,
		NavNodeKind::File(_) => 3,
		NavNodeKind::Def(_) => 4,
	};
	(group, node.label.as_str())
}

fn compute_nav_counts(node: &mut NavNode) -> (usize, usize) {
	let mut files = usize::from(matches!(node.kind, NavNodeKind::File(_)));
	let mut defs = usize::from(matches!(node.kind, NavNodeKind::Def(_)));
	for child in &mut node.children {
		let (child_files, child_defs) = compute_nav_counts(child);
		files += child_files;
		defs += child_defs;
	}
	node.file_count = files;
	node.def_count = defs;
	(files, defs)
}

fn symbol_children(
	index: &SessionIndex,
	file_idx: usize,
	parent: Option<DefLocation>,
) -> Vec<NavNode> {
	let mut out = Vec::new();
	for loc in direct_children(index, file_idx, parent) {
		collect_symbol_node(index, file_idx, loc, &mut out);
	}
	out
}

fn collect_symbol_node(
	index: &SessionIndex,
	file_idx: usize,
	loc: DefLocation,
	out: &mut Vec<NavNode>,
) {
	let def = index.def(&loc);
	if is_nav_symbol(def) {
		let mut node = NavNode::new(
			format!("def:{}:{}", loc.file, loc.def),
			last_name(&def.moniker),
			NavNodeKind::Def(loc),
		);
		node.children = symbol_children(index, file_idx, Some(loc));
		out.push(node);
	} else {
		out.extend(symbol_children(index, file_idx, Some(loc)));
	}
}

fn direct_children(
	index: &SessionIndex,
	file_idx: usize,
	parent: Option<DefLocation>,
) -> Vec<DefLocation> {
	let mut locs: Vec<DefLocation> = if let Some(parent) = parent {
		index
			.children_by_parent
			.get(&index.def(&parent).moniker)
			.into_iter()
			.flat_map(|children| children.iter().copied())
			.filter(|loc| loc.file == file_idx)
			.collect()
	} else {
		index.files[file_idx]
			.graph
			.defs()
			.enumerate()
			.filter(|(_, def)| def.parent.is_none())
			.map(|(def_idx, _)| DefLocation {
				file: file_idx,
				def: def_idx,
			})
			.collect()
	};
	locs.sort_by(|a, b| {
		let left = index.def(a);
		let right = index.def(b);
		left.position
			.map(|(start, _)| start)
			.cmp(&right.position.map(|(start, _)| start))
			.then_with(|| last_name(&left.moniker).cmp(&last_name(&right.moniker)))
	});
	locs
}

pub(super) fn is_nav_symbol(def: &DefRecord) -> bool {
	matches!(
		def_kind(def).as_str(),
		"annotation_type"
			| "class" | "const"
			| "constructor"
			| "enum" | "enum_constant"
			| "field" | "fn"
			| "func" | "function"
			| "impl" | "interface"
			| "method"
			| "record"
			| "struct"
			| "test" | "trait"
			| "type" | "var"
	)
}

pub(super) fn flatten_nav(
	index: &SessionIndex,
	node: &NavNode,
	expanded: &BTreeSet<String>,
	filter: Option<&NavFilter>,
	depth: usize,
	rows: &mut Vec<NavRow>,
) {
	if let Some(filter) = filter {
		let Some(filtered) = filter_node(index, node, filter) else {
			return;
		};
		for child in &filtered.children {
			flatten_compact_nav(child, expanded, depth, rows);
		}
		return;
	}
	for child in &node.children {
		flatten_compact_nav(child, expanded, depth, rows);
	}
}

fn flatten_compact_nav(
	node: &NavNode,
	expanded: &BTreeSet<String>,
	depth: usize,
	rows: &mut Vec<NavRow>,
) {
	let (rendered, label) = compact_chain(node);
	rows.push(NavRow {
		key: rendered.key.clone(),
		label,
		kind: rendered.kind,
		depth,
		has_children: !rendered.children.is_empty(),
		file_count: node.file_count,
		def_count: node.def_count,
	});
	if !rendered.children.is_empty() && expanded.contains(&rendered.key) {
		for child in &rendered.children {
			flatten_compact_nav(child, expanded, depth + 1, rows);
		}
	}
}

fn compact_chain(node: &NavNode) -> (&NavNode, String) {
	let mut current = node;
	let mut labels = vec![node.label.clone()];
	while !matches!(current.kind, NavNodeKind::Def(_)) && current.children.len() == 1 {
		let Some(child) = current.children.first() else {
			break;
		};
		labels.push(child.label.clone());
		current = child;
	}
	(current, labels.join("/"))
}

pub(super) fn filtered_expanded_keys(
	index: &SessionIndex,
	node: &NavNode,
	filter: &NavFilter,
	expand_symbols: bool,
) -> BTreeSet<String> {
	let mut keys = BTreeSet::new();
	for child in &node.children {
		collect_filtered_expanded_keys(index, child, filter, expand_symbols, &mut keys);
	}
	keys
}

fn filter_node(index: &SessionIndex, node: &NavNode, filter: &NavFilter) -> Option<NavNode> {
	let mut children = Vec::new();
	let mut files = 0;
	let mut defs = usize::from(node_matches(index, node, filter));
	for child in &node.children {
		if let Some(child) = filter_node(index, child, filter) {
			files += child.file_count;
			defs += child.def_count;
			children.push(child);
		}
	}
	if matches!(node.kind, NavNodeKind::File(_)) && (!children.is_empty() || defs > 0) {
		files = 1;
	}
	if children.is_empty() && defs == 0 {
		return None;
	}
	Some(NavNode {
		key: node.key.clone(),
		label: node.label.clone(),
		kind: node.kind,
		children,
		file_count: files,
		def_count: defs,
	})
}

fn collect_filtered_expanded_keys(
	index: &SessionIndex,
	node: &NavNode,
	filter: &NavFilter,
	expand_symbols: bool,
	keys: &mut BTreeSet<String>,
) -> Option<bool> {
	let mut has_matching_child = false;
	for child in &node.children {
		if collect_filtered_expanded_keys(index, child, filter, expand_symbols, keys).is_some() {
			has_matching_child = true;
		}
	}
	let included = node_matches(index, node, filter) || has_matching_child;
	if !included {
		return None;
	}
	if has_matching_child
		&& (expand_symbols || matches!(node.kind, NavNodeKind::Lang | NavNodeKind::Dir))
	{
		keys.insert(node.key.clone());
	}
	Some(has_matching_child)
}

fn node_matches(index: &SessionIndex, node: &NavNode, filter: &NavFilter) -> bool {
	let NavNodeKind::Def(loc) = node.kind else {
		return false;
	};
	let def = index.def(&loc);
	filter.matches(&def_kind(def), &last_name(&def.moniker))
}
