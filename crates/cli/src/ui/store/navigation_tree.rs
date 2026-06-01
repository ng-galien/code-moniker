use std::collections::BTreeSet;

use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;
use code_moniker_workspace::snapshot::{
	ChangeId, SourceFileRecord, SourceId, SymbolId, SymbolRecord, WorkspaceSnapshot,
};
use rustc_hash::FxHashMap;
type DefLocation = SymbolId;

use crate::ui::store::ids::NodeId;
use crate::ui::workspace_read::{self, LocalWorkspaceRegistry};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum NavNodeKind {
	Root,
	Lang,
	Dir,
	File(usize),
	Def(DefLocation),
	ChangeFile,
	Change(ChangeId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct NavNode {
	pub(in crate::ui) key: NodeId,
	pub(in crate::ui) label: String,
	pub(in crate::ui) kind: NavNodeKind,
	pub(in crate::ui) children: Vec<NavNode>,
	pub(in crate::ui) file_count: usize,
	pub(in crate::ui) def_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct NavRow {
	pub(in crate::ui) key: NodeId,
	pub(in crate::ui) label: String,
	pub(in crate::ui) kind: NavNodeKind,
	pub(in crate::ui) depth: usize,
	pub(in crate::ui) has_children: bool,
	pub(in crate::ui) file_count: usize,
	pub(in crate::ui) def_count: usize,
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
}

pub(in crate::ui) fn build_navigator(store: &LocalWorkspaceRegistry) -> NavNode {
	let mut root = NavNode::new(NodeId::root("explorer"), "root", NavNodeKind::Root);
	let Some(snapshot) = store.queries().snapshot() else {
		return root;
	};
	let symbols = SymbolNavIndex::new(snapshot);
	for (file_idx, file) in snapshot.index.sources.iter().enumerate() {
		let lang = Lang::from_tag(&file.language).unwrap_or(Lang::Rs);
		let lang = child_mut(
			&mut root,
			NodeId::lang("explorer", lang.tag()),
			lang.tag().to_string(),
			NavNodeKind::Lang,
		);
		append_source_file(lang, file_idx, file, &symbols);
	}
	sort_nav(&mut root);
	compute_nav_counts(&mut root);
	root
}

fn append_source_file(
	lang_node: &mut NavNode,
	file_idx: usize,
	file: &SourceFileRecord,
	symbols: &SymbolNavIndex<'_>,
) {
	let lang = Lang::from_tag(&file.language).unwrap_or(Lang::Rs);
	let rel_path = std::path::Path::new(&file.rel_path);
	let anchor = std::path::Path::new(&file.anchor);
	let parent = append_path_dirs(lang_node, lang, rel_path);
	let mut file_node = NavNode::new(
		NodeId::file(anchor),
		source_file_label(rel_path),
		NavNodeKind::File(file_idx),
	);
	file_node.children = symbols.children_for(&file.id, None);
	parent.children.push(file_node);
}

fn append_path_dirs<'a>(
	lang_node: &'a mut NavNode,
	lang: Lang,
	rel_path: &std::path::Path,
) -> &'a mut NavNode {
	let mut parent = lang_node;
	let mut path_key = String::new();
	for part in path_parts(rel_path) {
		if !path_key.is_empty() {
			path_key.push('/');
		}
		path_key.push_str(&part);
		parent = child_mut(
			parent,
			NodeId::dir("explorer", lang.tag(), &path_key),
			part,
			NavNodeKind::Dir,
		);
	}
	parent
}

fn path_parts(path: &std::path::Path) -> impl Iterator<Item = String> + '_ {
	path.parent()
		.into_iter()
		.flat_map(|path| path.components())
		.filter_map(|component| component.as_os_str().to_str())
		.map(ToOwned::to_owned)
}

fn source_file_label(rel_path: &std::path::Path) -> String {
	rel_path
		.file_name()
		.and_then(|name| name.to_str())
		.unwrap_or_else(|| rel_path.to_str().unwrap_or("<file>"))
		.to_string()
}

pub(in crate::ui) fn build_change_navigator(store: &LocalWorkspaceRegistry) -> NavNode {
	let mut root = NavNode::new(NodeId::root("change"), "root", NavNodeKind::Root);
	for change in workspace_read::change_rows(store) {
		let lang = child_mut(
			&mut root,
			NodeId::lang("change", change.lang.tag()),
			change.lang.tag().to_string(),
			NavNodeKind::Lang,
		);
		let mut parent = lang;
		let path_parts: Vec<String> = change
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
				NodeId::dir("change", change.lang.tag(), &path_key),
				part,
				NavNodeKind::Dir,
			);
		}
		let file_label = change
			.file_path
			.file_name()
			.and_then(|name| name.to_str())
			.unwrap_or_else(|| change.file_path.to_str().unwrap_or("<file>"))
			.to_string();
		let file = child_mut(
			parent,
			NodeId::change_file(&change.file_path),
			file_label,
			NavNodeKind::ChangeFile,
		);
		file.children.push(NavNode::new(
			NodeId::change(&change.compact_moniker),
			change.name.clone(),
			NavNodeKind::Change(change.id),
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

struct SymbolNavIndex<'a> {
	by_parent: FxHashMap<(SourceId, Option<SymbolId>), Vec<&'a SymbolRecord>>,
}

impl<'a> SymbolNavIndex<'a> {
	fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		let by_id = symbols_by_id(&snapshot.index.symbols);
		let source_langs = source_langs(snapshot);
		let mut by_parent = FxHashMap::default();
		for symbol in &snapshot.index.symbols {
			if !symbol.navigable {
				continue;
			}
			let parent = navigable_parent(symbol, &by_id);
			by_parent
				.entry((symbol.source.clone(), parent))
				.or_insert_with(Vec::new)
				.push(symbol);
		}
		sort_symbol_groups(&mut by_parent, &source_langs);
		Self { by_parent }
	}

	fn children_for(&self, source: &SourceId, parent: Option<&SymbolId>) -> Vec<NavNode> {
		let key = (source.clone(), parent.cloned());
		self.by_parent
			.get(&key)
			.into_iter()
			.flat_map(|symbols| symbols.iter())
			.map(|symbol| symbol_nav_node(self, source, symbol))
			.collect()
	}
}

fn symbol_nav_node(
	index: &SymbolNavIndex<'_>,
	source: &SourceId,
	symbol: &SymbolRecord,
) -> NavNode {
	let mut node = NavNode::new(
		NodeId::def(symbol.id.as_str()),
		symbol.name.clone(),
		NavNodeKind::Def(symbol.id.clone()),
	);
	node.children = index.children_for(source, Some(&symbol.id));
	node
}

fn symbols_by_id(symbols: &[SymbolRecord]) -> FxHashMap<SymbolId, &SymbolRecord> {
	symbols
		.iter()
		.map(|symbol| (symbol.id.clone(), symbol))
		.collect()
}

fn source_langs(snapshot: &WorkspaceSnapshot) -> FxHashMap<SourceId, Lang> {
	snapshot
		.index
		.sources
		.iter()
		.map(|source| {
			(
				source.id.clone(),
				Lang::from_tag(&source.language).unwrap_or(Lang::Rs),
			)
		})
		.collect()
}

fn navigable_parent(
	symbol: &SymbolRecord,
	by_id: &FxHashMap<SymbolId, &SymbolRecord>,
) -> Option<SymbolId> {
	let parent_id = symbol.parent.as_ref()?;
	let parent = by_id.get(parent_id)?;
	(parent.navigable && parent.source == symbol.source).then(|| parent_id.clone())
}

fn sort_symbol_groups(
	by_parent: &mut FxHashMap<(SourceId, Option<SymbolId>), Vec<&SymbolRecord>>,
	source_langs: &FxHashMap<SourceId, Lang>,
) {
	for ((source, _), symbols) in by_parent {
		let lang = source_langs.get(source).copied().unwrap_or(Lang::Rs);
		symbols.sort_by(|left, right| compare_symbols_for_navigation(lang, left, right));
	}
}

fn compare_symbols_for_navigation(
	lang: Lang,
	left: &SymbolRecord,
	right: &SymbolRecord,
) -> std::cmp::Ordering {
	definition_kind_order(lang, &left.kind)
		.cmp(&definition_kind_order(lang, &right.kind))
		.then_with(|| left.line_range.cmp(&right.line_range))
		.then_with(|| left.name.cmp(&right.name))
}

fn definition_kind_order(lang: Lang, kind: &str) -> u16 {
	lang.kind_spec(kind)
		.map(|spec| spec.order)
		.or_else(|| shape_of(kind.as_bytes()).map(fallback_order_for_shape))
		.unwrap_or(u16::MAX)
}

fn fallback_order_for_shape(shape: Shape) -> u16 {
	match shape {
		Shape::Namespace => 10,
		Shape::Type => 20,
		Shape::Callable => 30,
		Shape::Value => 50,
		Shape::Annotation => 60,
		Shape::Ref => u16::MAX,
	}
}

pub(in crate::ui) fn flatten_nav(
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
		kind: rendered.kind.clone(),
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

pub(in crate::ui) fn filtered_expanded_keys(
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
		kind: node.kind.clone(),
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
	let NavNodeKind::Def(loc) = &node.kind else {
		return false;
	};
	matches.contains(loc)
}

pub(in crate::ui) fn all_expanded_keys(node: &NavNode) -> BTreeSet<NodeId> {
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
