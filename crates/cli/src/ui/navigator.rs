use std::collections::BTreeSet;

use crate::inspect::DefLocation;

use super::store::ids::NodeId;
use super::store::{IndexStore, is_navigable_def, last_name};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NavNodeKind {
	Root,
	Lang,
	Dir,
	File(usize),
	Def(DefLocation),
	ChangeFile,
	Change(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NavNode {
	pub(super) key: NodeId,
	pub(super) label: String,
	pub(super) kind: NavNodeKind,
	pub(super) children: Vec<NavNode>,
	pub(super) file_count: usize,
	pub(super) def_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NavRow {
	pub(super) key: NodeId,
	pub(super) label: String,
	pub(super) kind: NavNodeKind,
	pub(super) depth: usize,
	pub(super) has_children: bool,
	pub(super) file_count: usize,
	pub(super) def_count: usize,
}

impl NavNode {
	pub(in crate::ui) fn new(key: NodeId, label: impl Into<String>, kind: NavNodeKind) -> Self {
		Self {
			key,
			label: label.into(),
			kind,
			children: Vec::new(),
			file_count: 0,
			def_count: 0,
		}
	}

	#[cfg(test)]
	pub(in crate::ui) fn push_child(&mut self, child: NavNode) {
		self.children.push(child);
		compute_nav_counts(self);
	}
}

pub(super) fn build_navigator(store: &impl IndexStore) -> NavNode {
	let mut root = NavNode::new(NodeId::root("explorer"), "root", NavNodeKind::Root);
	for file_idx in 0..store.file_count() {
		let file = store.file(file_idx);
		let lang = child_mut(
			&mut root,
			NodeId::lang("explorer", file.lang.tag()),
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
				NodeId::dir("explorer", file.lang.tag(), &path_key),
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
			NodeId::file(&file.anchor),
			file_label,
			NavNodeKind::File(file_idx),
		);
		file_node.children = symbol_children(store, file_idx, None);
		parent.children.push(file_node);
	}
	sort_nav(&mut root);
	compute_nav_counts(&mut root);
	root
}

pub(super) fn build_change_navigator(store: &impl IndexStore) -> NavNode {
	let mut root = NavNode::new(NodeId::root("change"), "root", NavNodeKind::Root);
	for (change_idx, entry) in store.change_index().entries.iter().enumerate() {
		let lang = child_mut(
			&mut root,
			NodeId::lang("change", entry.lang.tag()),
			entry.lang.tag().to_string(),
			NavNodeKind::Lang,
		);
		let mut parent = lang;
		let path_parts: Vec<String> = entry
			.file_path
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
				NodeId::dir("change", entry.lang.tag(), &path_key),
				part,
				NavNodeKind::Dir,
			);
		}
		let file_label = entry
			.file_path
			.file_name()
			.and_then(|name| name.to_str())
			.unwrap_or_else(|| entry.file_path.to_str().unwrap_or("<file>"))
			.to_string();
		let file = child_mut(
			parent,
			NodeId::change_file(&entry.file_path),
			file_label,
			NavNodeKind::ChangeFile,
		);
		file.children.push(NavNode::new(
			NodeId::change(&entry.moniker),
			entry.name.clone(),
			NavNodeKind::Change(change_idx),
		));
	}
	sort_nav(&mut root);
	compute_nav_counts(&mut root);
	root
}

fn child_mut(node: &mut NavNode, key: NodeId, label: String, kind: NavNodeKind) -> &mut NavNode {
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
	if should_sort_children(node) {
		node.children
			.sort_by(|a, b| nav_sort_key(a).cmp(&nav_sort_key(b)));
	}
	for child in &mut node.children {
		sort_nav(child);
	}
}

fn should_sort_children(node: &NavNode) -> bool {
	!matches!(node.kind, NavNodeKind::File(_) | NavNodeKind::Def(_))
}

fn nav_sort_key(node: &NavNode) -> (u8, &str) {
	let group = match node.kind {
		NavNodeKind::Root => 0,
		NavNodeKind::Lang => 1,
		NavNodeKind::Dir => 2,
		NavNodeKind::File(_) | NavNodeKind::ChangeFile => 3,
		NavNodeKind::Def(_) | NavNodeKind::Change(_) => 4,
	};
	(group, node.label.as_str())
}

fn compute_nav_counts(node: &mut NavNode) -> (usize, usize) {
	let mut files = usize::from(matches!(
		node.kind,
		NavNodeKind::File(_) | NavNodeKind::ChangeFile
	));
	let mut defs = usize::from(matches!(
		node.kind,
		NavNodeKind::Def(_) | NavNodeKind::Change(_)
	));
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
	store: &impl IndexStore,
	file_idx: usize,
	parent: Option<DefLocation>,
) -> Vec<NavNode> {
	let mut out = Vec::new();
	for loc in direct_children(store, file_idx, parent) {
		collect_symbol_node(store, file_idx, loc, &mut out);
	}
	sort_symbol_nodes(store, &mut out);
	out
}

fn sort_symbol_nodes(store: &impl IndexStore, nodes: &mut [NavNode]) {
	nodes.sort_by(|a, b| match (a.kind, b.kind) {
		(NavNodeKind::Def(left), NavNodeKind::Def(right)) => {
			store.compare_defs_for_navigation(&left, &right)
		}
		_ => nav_sort_key(a).cmp(&nav_sort_key(b)),
	});
}

fn collect_symbol_node(
	store: &impl IndexStore,
	file_idx: usize,
	loc: DefLocation,
	out: &mut Vec<NavNode>,
) {
	let def = store.def(&loc);
	if is_navigable_def(store.file(file_idx).lang, def) {
		let mut node = NavNode::new(
			NodeId::def(&def.moniker),
			last_name(&def.moniker),
			NavNodeKind::Def(loc),
		);
		node.children = symbol_children(store, file_idx, Some(loc));
		out.push(node);
	} else {
		out.extend(symbol_children(store, file_idx, Some(loc)));
	}
}

fn direct_children(
	store: &impl IndexStore,
	file_idx: usize,
	parent: Option<DefLocation>,
) -> Vec<DefLocation> {
	if let Some(parent) = parent {
		store.child_defs(&parent)
	} else {
		store.root_defs(file_idx)
	}
}

pub(super) fn flatten_nav(
	node: &NavNode,
	expanded: &BTreeSet<NodeId>,
	matches: Option<&[DefLocation]>,
	depth: usize,
	rows: &mut Vec<NavRow>,
) {
	if let Some(matches) = matches {
		let Some(filtered) = filter_node(node, matches) else {
			return;
		};
		for child in &filtered.children {
			flatten_compact_nav(child, expanded, depth, rows, true);
		}
		return;
	}
	for child in &node.children {
		flatten_compact_nav(child, expanded, depth, rows, false);
	}
}

fn flatten_compact_nav(
	node: &NavNode,
	expanded: &BTreeSet<NodeId>,
	depth: usize,
	rows: &mut Vec<NavRow>,
	allow_terminal_compaction: bool,
) {
	let (rendered, label) = compact_chain(node, allow_terminal_compaction);
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
			flatten_compact_nav(child, expanded, depth + 1, rows, allow_terminal_compaction);
		}
	}
}

fn compact_chain(node: &NavNode, allow_terminal_compaction: bool) -> (&NavNode, String) {
	let mut current = node;
	let mut labels = vec![node.label.clone()];
	while !matches!(current.kind, NavNodeKind::Def(_)) && current.children.len() == 1 {
		let Some(child) = current.children.first() else {
			break;
		};
		if !allow_terminal_compaction
			&& matches!(
				child.kind,
				NavNodeKind::File(_)
					| NavNodeKind::Def(_)
					| NavNodeKind::ChangeFile
					| NavNodeKind::Change(_)
			) {
			break;
		}
		labels.push(child.label.clone());
		current = child;
	}
	(current, labels.join("/"))
}

pub(super) fn filtered_expanded_keys(
	node: &NavNode,
	matches: &[DefLocation],
	expand_symbols: bool,
) -> BTreeSet<NodeId> {
	let mut keys = BTreeSet::new();
	for child in &node.children {
		collect_filtered_expanded_keys(child, matches, expand_symbols, &mut keys);
	}
	keys
}

fn filter_node(node: &NavNode, matches: &[DefLocation]) -> Option<NavNode> {
	let mut children = Vec::new();
	let mut files = 0;
	let mut defs = usize::from(node_matches(node, matches));
	for child in &node.children {
		if let Some(child) = filter_node(child, matches) {
			files += child.file_count;
			defs += child.def_count;
			children.push(child);
		}
	}
	if matches!(node.kind, NavNodeKind::File(_) | NavNodeKind::ChangeFile)
		&& (!children.is_empty() || defs > 0)
	{
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
	node: &NavNode,
	matches: &[DefLocation],
	expand_symbols: bool,
	keys: &mut BTreeSet<NodeId>,
) -> Option<bool> {
	let mut has_matching_child = false;
	for child in &node.children {
		if collect_filtered_expanded_keys(child, matches, expand_symbols, keys).is_some() {
			has_matching_child = true;
		}
	}
	let included = node_matches(node, matches) || has_matching_child;
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

fn node_matches(node: &NavNode, matches: &[DefLocation]) -> bool {
	let NavNodeKind::Def(loc) = node.kind else {
		return false;
	};
	matches.contains(&loc)
}

pub(super) fn all_expanded_keys(node: &NavNode) -> BTreeSet<NodeId> {
	let mut keys = BTreeSet::new();
	collect_all_expanded_keys(node, &mut keys);
	keys
}

fn collect_all_expanded_keys(node: &NavNode, keys: &mut BTreeSet<NodeId>) {
	if !node.children.is_empty() {
		keys.insert(node.key.clone());
	}
	for child in &node.children {
		collect_all_expanded_keys(child, keys);
	}
}
