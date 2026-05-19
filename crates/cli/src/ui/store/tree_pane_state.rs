use std::collections::BTreeSet;

use crate::ui::navigator::NavRow;
use crate::ui::store::ids::NodeId;

use super::tree_pane_action::{TreePaneAction, TreePaneNotice};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct TreePaneState {
	expanded: BTreeSet<NodeId>,
	rows: Vec<NavRow>,
	selection: usize,
}

impl TreePaneState {
	pub(in crate::ui) fn new() -> Self {
		Self {
			expanded: BTreeSet::new(),
			rows: Vec::new(),
			selection: 0,
		}
	}

	pub(in crate::ui) fn rows(&self) -> &[NavRow] {
		&self.rows
	}

	pub(in crate::ui) fn selection(&self) -> usize {
		self.selection
	}

	pub(in crate::ui) fn selected_row(&self) -> Option<&NavRow> {
		self.rows.get(self.selection)
	}

	pub(in crate::ui) fn selected_key(&self) -> Option<NodeId> {
		self.selected_row().map(|row| row.key.clone())
	}

	pub(in crate::ui) fn expanded(&self) -> &BTreeSet<NodeId> {
		&self.expanded
	}

	pub(in crate::ui) fn set_expanded(&mut self, expanded: BTreeSet<NodeId>) {
		self.expanded = expanded;
	}

	pub(in crate::ui) fn retain_expanded(&mut self, valid: &BTreeSet<NodeId>) {
		self.expanded.retain(|key| valid.contains(key));
	}

	pub(in crate::ui) fn reset_selection(&mut self) {
		self.selection = 0;
	}

	pub(in crate::ui) fn replace_rows(&mut self, rows: Vec<NavRow>, selected_key: Option<&NodeId>) {
		self.rows = rows;
		self.restore_selection(selected_key);
	}

	pub(in crate::ui) fn select_first_matching(
		&mut self,
		mut predicate: impl FnMut(&NavRow) -> bool,
	) {
		if let Some(idx) = self.rows.iter().position(|row| predicate(row)) {
			self.selection = idx;
		}
	}

	pub(in crate::ui) fn apply(&mut self, action: TreePaneAction) -> TreePaneNotice {
		match action {
			TreePaneAction::MoveDown => self.move_down(),
			TreePaneAction::MoveUp => self.move_up(),
			TreePaneAction::Home => self.home(),
			TreePaneAction::End => self.end(),
			TreePaneAction::ToggleSelected => return self.toggle_selected(),
			TreePaneAction::OpenSelected => return self.open_selected(),
			TreePaneAction::CloseSelected => return self.close_selected(),
		}
		TreePaneNotice::Noop
	}

	fn restore_selection(&mut self, key: Option<&NodeId>) {
		let Some(key) = key else {
			self.clamp_selection();
			return;
		};
		if let Some(idx) = self.rows.iter().position(|row| &row.key == key) {
			self.selection = idx;
		} else {
			self.clamp_selection();
		}
	}

	fn clamp_selection(&mut self) {
		clamp_index(&mut self.selection, self.rows.len());
	}

	fn move_down(&mut self) {
		if self.selection + 1 < self.rows.len() {
			self.selection += 1;
		}
	}

	fn move_up(&mut self) {
		self.selection = self.selection.saturating_sub(1);
	}

	fn home(&mut self) {
		self.selection = 0;
	}

	fn end(&mut self) {
		self.selection = self.rows.len().saturating_sub(1);
	}

	fn toggle_selected(&mut self) -> TreePaneNotice {
		let Some(row) = self.selected_row().cloned() else {
			return TreePaneNotice::Noop;
		};
		if !row.has_children {
			return TreePaneNotice::Noop;
		}
		if self.expanded.remove(&row.key) {
			TreePaneNotice::Closed(row.label)
		} else {
			self.expanded.insert(row.key);
			TreePaneNotice::Opened(row.label)
		}
	}

	fn open_selected(&mut self) -> TreePaneNotice {
		let Some(row) = self.selected_row().cloned() else {
			return TreePaneNotice::Noop;
		};
		if row.has_children && !self.expanded.contains(&row.key) {
			self.expanded.insert(row.key);
			return TreePaneNotice::Opened(row.label);
		}
		TreePaneNotice::Noop
	}

	fn close_selected(&mut self) -> TreePaneNotice {
		let Some(row) = self.selected_row().cloned() else {
			return TreePaneNotice::Noop;
		};
		if row.has_children && self.expanded.contains(&row.key) {
			self.expanded.remove(&row.key);
			return TreePaneNotice::Closed(row.label);
		}
		if row.depth == 0 {
			return TreePaneNotice::Noop;
		}
		let parent_depth = row.depth - 1;
		if let Some(parent) = self.rows[..self.selection]
			.iter()
			.rposition(|candidate| candidate.depth == parent_depth)
		{
			self.selection = parent;
			return TreePaneNotice::MovedToParent;
		}
		TreePaneNotice::Noop
	}
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
	use crate::ui::navigator::NavNodeKind;

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
