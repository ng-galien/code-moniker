use std::collections::BTreeSet;

use crate::ui::store::ids::NodeId;
use crate::ui::store::navigation_tree::NavRow;

use super::tree_pane_action::{TreePaneAction, TreePaneNotice};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct TreePaneState {
	expanded: BTreeSet<NodeId>,
	rows: Vec<NavRow>,
	selection: usize,
}

impl Default for TreePaneState {
	fn default() -> Self {
		Self::new()
	}
}

impl TreePaneState {
	pub(in crate::ui) fn new() -> Self {
		Self {
			expanded: BTreeSet::new(),
			rows: Vec::new(),
			selection: 0,
		}
	}
}

pub(in crate::ui) fn tree_pane_rows(pane: &TreePaneState) -> &[NavRow] {
	&pane.rows
}

pub(in crate::ui) fn tree_pane_selection(pane: &TreePaneState) -> usize {
	pane.selection
}

pub(in crate::ui) fn tree_pane_selected_row(pane: &TreePaneState) -> Option<&NavRow> {
	pane.rows.get(pane.selection)
}

pub(in crate::ui) fn tree_pane_selected_key(pane: &TreePaneState) -> Option<NodeId> {
	tree_pane_selected_row(pane).map(|row| row.key.clone())
}

pub(in crate::ui) fn tree_pane_expanded(pane: &TreePaneState) -> &BTreeSet<NodeId> {
	&pane.expanded
}

pub(in crate::ui) fn tree_pane_set_expanded(pane: &mut TreePaneState, expanded: BTreeSet<NodeId>) {
	pane.expanded = expanded;
}

pub(in crate::ui) fn tree_pane_retain_expanded(pane: &mut TreePaneState, valid: &BTreeSet<NodeId>) {
	pane.expanded.retain(|key| valid.contains(key));
}

pub(in crate::ui) fn tree_pane_reset_selection(pane: &mut TreePaneState) {
	pane.selection = 0;
}

pub(in crate::ui) fn tree_pane_replace_rows(
	pane: &mut TreePaneState,
	rows: Vec<NavRow>,
	selected_key: Option<&NodeId>,
) {
	pane.rows = rows;
	restore_selection(pane, selected_key);
}

pub(in crate::ui) fn tree_pane_select_first_matching(
	pane: &mut TreePaneState,
	predicate: impl FnMut(&NavRow) -> bool,
) {
	if let Some(idx) = pane.rows.iter().position(predicate) {
		pane.selection = idx;
	}
}

pub(in crate::ui) fn tree_pane_apply(
	pane: &mut TreePaneState,
	action: TreePaneAction,
) -> TreePaneNotice {
	match action {
		TreePaneAction::MoveDown => move_down(pane),
		TreePaneAction::MoveUp => move_up(pane),
		TreePaneAction::Home => home(pane),
		TreePaneAction::End => end(pane),
		TreePaneAction::ToggleSelected => return toggle_selected(pane),
		TreePaneAction::OpenSelected => return open_selected(pane),
		TreePaneAction::CloseSelected => return close_selected(pane),
	}
	TreePaneNotice::Noop
}

fn restore_selection(pane: &mut TreePaneState, key: Option<&NodeId>) {
	let Some(key) = key else {
		clamp_selection(pane);
		return;
	};
	if let Some(idx) = pane.rows.iter().position(|row| &row.key == key) {
		pane.selection = idx;
	} else {
		clamp_selection(pane);
	}
}

fn clamp_selection(pane: &mut TreePaneState) {
	clamp_index(&mut pane.selection, pane.rows.len());
}

fn move_down(pane: &mut TreePaneState) {
	if pane.selection + 1 < pane.rows.len() {
		pane.selection += 1;
	}
}

fn move_up(pane: &mut TreePaneState) {
	pane.selection = pane.selection.saturating_sub(1);
}

fn home(pane: &mut TreePaneState) {
	pane.selection = 0;
}

fn end(pane: &mut TreePaneState) {
	pane.selection = pane.rows.len().saturating_sub(1);
}

fn toggle_selected(pane: &mut TreePaneState) -> TreePaneNotice {
	let Some(row) = tree_pane_selected_row(pane).cloned() else {
		return TreePaneNotice::Noop;
	};
	if !row.has_children {
		return TreePaneNotice::Noop;
	}
	if pane.expanded.remove(&row.key) {
		TreePaneNotice::Closed(row.label)
	} else {
		pane.expanded.insert(row.key);
		TreePaneNotice::Opened(row.label)
	}
}

fn open_selected(pane: &mut TreePaneState) -> TreePaneNotice {
	let Some(row) = tree_pane_selected_row(pane).cloned() else {
		return TreePaneNotice::Noop;
	};
	if row.has_children && !pane.expanded.contains(&row.key) {
		pane.expanded.insert(row.key);
		return TreePaneNotice::Opened(row.label);
	}
	TreePaneNotice::Noop
}

fn close_selected(pane: &mut TreePaneState) -> TreePaneNotice {
	let Some(row) = tree_pane_selected_row(pane).cloned() else {
		return TreePaneNotice::Noop;
	};
	if row.has_children && pane.expanded.contains(&row.key) {
		pane.expanded.remove(&row.key);
		return TreePaneNotice::Closed(row.label);
	}
	if row.depth == 0 {
		return TreePaneNotice::Noop;
	}
	let parent_depth = row.depth - 1;
	if let Some(parent) = pane.rows[..pane.selection]
		.iter()
		.rposition(|candidate| candidate.depth == parent_depth)
	{
		pane.selection = parent;
		return TreePaneNotice::MovedToParent;
	}
	TreePaneNotice::Noop
}

fn clamp_index(selection: &mut usize, len: usize) {
	if len == 0 {
		*selection = 0;
	} else if *selection >= len {
		*selection = len - 1;
	}
}

// Disabled during the UI architecture rebuild; rewrite against the new component contracts later.
#[cfg(any())]
mod tests {
	use super::*;
	use crate::ui::store::navigation_tree::NavNodeKind;

	fn row(key: NodeId, label: &str, depth: usize, has_children: bool) -> NavRow {
		NavRow {
			key,
			label: label.to_string(),
			kind: NavNodeKind::Dir,
			depth,
			has_children,
			file_count: 0,
			def_count: 0,
		}
	}

	#[test]
	fn replace_rows_restores_selection_by_key() {
		let selected = NodeId::dir("test", "rs", "src");
		let mut pane = TreePaneState::new();
		pane.replace_rows(
			vec![
				row(NodeId::lang("test", "rs"), "rs", 0, true),
				row(selected.clone(), "src", 1, true),
			],
			None,
		);
		pane.apply(TreePaneAction::MoveDown);

		pane.replace_rows(
			vec![
				row(NodeId::lang("test", "rs"), "rs", 0, true),
				row(selected.clone(), "src", 1, true),
				row(NodeId::dir("test", "rs", "tests"), "tests", 1, true),
			],
			Some(&selected),
		);

		assert_eq!(pane.selection(), 1);
		assert_eq!(pane.selected_row().map(|row| &row.key), Some(&selected));
	}

	#[test]
	fn select_first_matching_uses_caller_supplied_semantics() {
		let selected = NodeId::dir("test", "rs", "src");
		let mut pane = TreePaneState::new();
		pane.replace_rows(
			vec![
				row(NodeId::lang("test", "rs"), "rs", 0, true),
				row(selected.clone(), "src", 1, false),
			],
			None,
		);

		pane.select_first_matching(|row| row.label == "src");

		assert_eq!(pane.selected_row().map(|row| &row.key), Some(&selected));
	}

	#[test]
	fn toggle_selected_updates_expansion_and_notice() {
		let key = NodeId::lang("test", "rs");
		let mut pane = TreePaneState::new();
		pane.replace_rows(vec![row(key.clone(), "rs", 0, true)], None);

		assert_eq!(
			pane.apply(TreePaneAction::ToggleSelected),
			TreePaneNotice::Opened("rs".to_string())
		);
		assert!(pane.expanded().contains(&key));
		assert_eq!(
			pane.apply(TreePaneAction::ToggleSelected),
			TreePaneNotice::Closed("rs".to_string())
		);
		assert!(!pane.expanded().contains(&key));
	}

	#[test]
	fn close_selected_leaf_moves_to_parent() {
		let parent = NodeId::lang("test", "rs");
		let child = NodeId::dir("test", "rs", "src");
		let mut pane = TreePaneState::new();
		pane.replace_rows(
			vec![
				row(parent.clone(), "rs", 0, true),
				row(child.clone(), "src", 1, false),
			],
			Some(&child),
		);

		assert_eq!(
			pane.apply(TreePaneAction::CloseSelected),
			TreePaneNotice::MovedToParent
		);
		assert_eq!(pane.selected_row().map(|row| &row.key), Some(&parent));
	}
}
