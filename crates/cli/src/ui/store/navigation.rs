use std::collections::BTreeSet;

use crate::inspect::DefLocation;
use crate::ui::navigator::{
	NavNode, NavNodeKind, NavRow, all_expanded_keys, filtered_expanded_keys, flatten_nav,
};
use crate::ui::reactive::{Reduce, Transition};

use super::ids::NodeId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum NavigationScope {
	Explorer,
	Filtered,
	Change,
	Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum NavigationNotice {
	Opened(String),
	Closed(String),
	MovedToParent,
	Noop,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum NavigationAction {
	ReplaceModels {
		explorer: NavNode,
		change: NavNode,
	},
	SetScope {
		scope: NavigationScope,
		visible_defs: Vec<DefLocation>,
		reset_expansion: bool,
		expand_symbols: bool,
	},
	MoveDown,
	MoveUp,
	Home,
	End,
	SelectDef(DefLocation),
	SelectFirstChange,
	ToggleSelected,
	OpenSelected,
	CloseSelected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct NavigationState {
	explorer: NavNode,
	change: NavNode,
	expanded: BTreeSet<NodeId>,
	filtered_expanded: BTreeSet<NodeId>,
	rows: Vec<NavRow>,
	selection: usize,
	visible_defs: Vec<DefLocation>,
	scope: NavigationScope,
	last_notice: NavigationNotice,
}

impl NavigationState {
	pub(in crate::ui) fn new(explorer: NavNode, change: NavNode) -> Self {
		let mut state = Self {
			explorer,
			change,
			expanded: BTreeSet::new(),
			filtered_expanded: BTreeSet::new(),
			rows: Vec::new(),
			selection: 0,
			visible_defs: Vec::new(),
			scope: NavigationScope::Explorer,
			last_notice: NavigationNotice::Noop,
		};
		state.refresh_rows();
		state
	}

	pub(in crate::ui) fn rows(&self) -> &[NavRow] {
		&self.rows
	}

	pub(in crate::ui) fn visible_defs(&self) -> &[DefLocation] {
		&self.visible_defs
	}

	pub(in crate::ui) fn selection(&self) -> usize {
		self.selection
	}

	pub(in crate::ui) fn selected_row(&self) -> Option<&NavRow> {
		self.rows.get(self.selection)
	}

	pub(in crate::ui) fn explorer_def_count(&self) -> usize {
		self.explorer.def_count
	}

	pub(in crate::ui) fn active_expanded(&self) -> &BTreeSet<NodeId> {
		if self.is_filtered_scope() {
			&self.filtered_expanded
		} else {
			&self.expanded
		}
	}

	pub(in crate::ui) fn last_notice(&self) -> &NavigationNotice {
		&self.last_notice
	}

	fn active_expanded_mut(&mut self) -> &mut BTreeSet<NodeId> {
		if self.is_filtered_scope() {
			&mut self.filtered_expanded
		} else {
			&mut self.expanded
		}
	}

	fn is_filtered_scope(&self) -> bool {
		matches!(
			self.scope,
			NavigationScope::Filtered | NavigationScope::Change | NavigationScope::Invalid
		)
	}

	fn replace_models(&mut self, explorer: NavNode, change: NavNode) {
		let selected_key = self.selected_row().map(|row| row.key.clone());
		self.explorer = explorer;
		self.change = change;
		self.retain_valid_expansion();
		self.refresh_rows();
		self.restore_selection(selected_key.as_ref());
	}

	fn set_scope(
		&mut self,
		scope: NavigationScope,
		visible_defs: Vec<DefLocation>,
		reset_expansion: bool,
		expand_symbols: bool,
	) {
		self.scope = scope;
		self.visible_defs = visible_defs;
		if reset_expansion {
			self.filtered_expanded.clear();
			match self.scope {
				NavigationScope::Change => {
					self.filtered_expanded = all_expanded_keys(&self.change);
				}
				NavigationScope::Filtered => {
					self.filtered_expanded =
						filtered_expanded_keys(&self.explorer, &self.visible_defs, expand_symbols);
				}
				NavigationScope::Explorer | NavigationScope::Invalid => {}
			}
			self.selection = 0;
		}
		self.refresh_rows();
	}

	fn refresh_rows(&mut self) {
		self.rows.clear();
		match self.scope {
			NavigationScope::Invalid => {}
			NavigationScope::Change => {
				flatten_nav(
					&self.change,
					&self.filtered_expanded,
					None,
					0,
					&mut self.rows,
				);
			}
			NavigationScope::Filtered => {
				flatten_nav(
					&self.explorer,
					&self.filtered_expanded,
					Some(self.visible_defs.as_slice()),
					0,
					&mut self.rows,
				);
			}
			NavigationScope::Explorer => {
				flatten_nav(&self.explorer, &self.expanded, None, 0, &mut self.rows);
			}
		}
		self.clamp_selection();
	}

	fn retain_valid_expansion(&mut self) {
		let valid_explorer = all_expanded_keys(&self.explorer);
		self.expanded.retain(|key| valid_explorer.contains(key));
		let mut valid_filtered = valid_explorer;
		valid_filtered.extend(all_expanded_keys(&self.change));
		self.filtered_expanded
			.retain(|key| valid_filtered.contains(key));
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
		let len = self.rows.len();
		if len == 0 {
			self.selection = 0;
		} else if self.selection >= len {
			self.selection = len - 1;
		}
	}

	fn select_def(&mut self, loc: DefLocation) {
		if let Some(idx) = self
			.rows
			.iter()
			.position(|row| matches!(row.kind, NavNodeKind::Def(row_loc) if row_loc == loc))
		{
			self.selection = idx;
		}
	}

	fn select_first_change(&mut self) {
		if let Some(idx) = self
			.rows
			.iter()
			.position(|row| matches!(row.kind, NavNodeKind::Change(_)))
		{
			self.selection = idx;
		}
	}

	fn move_down(&mut self) {
		let len = self.rows.len();
		if self.selection + 1 < len {
			self.selection += 1;
		}
	}

	fn move_up(&mut self) {
		self.selection = self.selection.saturating_sub(1);
	}

	fn toggle_selected(&mut self) {
		let Some(row) = self.selected_row().cloned() else {
			return;
		};
		if !row.has_children {
			return;
		}
		if self.active_expanded_mut().remove(&row.key) {
			self.last_notice = NavigationNotice::Closed(row.label);
		} else {
			self.active_expanded_mut().insert(row.key);
			self.last_notice = NavigationNotice::Opened(row.label);
		}
		self.refresh_rows();
	}

	fn open_selected(&mut self) {
		let Some(row) = self.selected_row().cloned() else {
			return;
		};
		if row.has_children && !self.active_expanded().contains(&row.key) {
			self.active_expanded_mut().insert(row.key);
			self.last_notice = NavigationNotice::Opened(row.label);
			self.refresh_rows();
		}
	}

	fn close_selected(&mut self) {
		let Some(row) = self.selected_row().cloned() else {
			return;
		};
		if row.has_children && self.active_expanded().contains(&row.key) {
			self.active_expanded_mut().remove(&row.key);
			self.last_notice = NavigationNotice::Closed(row.label);
			self.refresh_rows();
			return;
		}
		if row.depth == 0 {
			return;
		}
		let parent_depth = row.depth - 1;
		if let Some(parent) = self.rows[..self.selection]
			.iter()
			.rposition(|candidate| candidate.depth == parent_depth)
		{
			self.selection = parent;
			self.last_notice = NavigationNotice::MovedToParent;
		}
	}
}

impl Reduce<NavigationAction> for NavigationState {
	fn reduce(&mut self, action: NavigationAction) -> Transition {
		self.last_notice = NavigationNotice::Noop;
		match action {
			NavigationAction::ReplaceModels { explorer, change } => {
				self.replace_models(explorer, change);
				Transition::changed("navigation.replace_models")
			}
			NavigationAction::SetScope {
				scope,
				visible_defs,
				reset_expansion,
				expand_symbols,
			} => {
				self.set_scope(scope, visible_defs, reset_expansion, expand_symbols);
				Transition::changed("navigation.set_scope")
			}
			NavigationAction::MoveDown => {
				let before = self.selection;
				self.move_down();
				if self.selection == before {
					Transition::unchanged("navigation.move_down")
				} else {
					Transition::changed("navigation.move_down")
				}
			}
			NavigationAction::MoveUp => {
				let before = self.selection;
				self.move_up();
				if self.selection == before {
					Transition::unchanged("navigation.move_up")
				} else {
					Transition::changed("navigation.move_up")
				}
			}
			NavigationAction::Home => {
				let before = self.selection;
				self.selection = 0;
				if self.selection == before {
					Transition::unchanged("navigation.home")
				} else {
					Transition::changed("navigation.home")
				}
			}
			NavigationAction::End => {
				let before = self.selection;
				self.selection = self.rows.len().saturating_sub(1);
				if self.selection == before {
					Transition::unchanged("navigation.end")
				} else {
					Transition::changed("navigation.end")
				}
			}
			NavigationAction::SelectDef(loc) => {
				let before = self.selection;
				self.select_def(loc);
				if self.selection == before {
					Transition::unchanged("navigation.select_def")
				} else {
					Transition::changed("navigation.select_def")
				}
			}
			NavigationAction::SelectFirstChange => {
				let before = self.selection;
				self.select_first_change();
				if self.selection == before {
					Transition::unchanged("navigation.select_first_change")
				} else {
					Transition::changed("navigation.select_first_change")
				}
			}
			NavigationAction::ToggleSelected => {
				self.toggle_selected();
				Transition::changed("navigation.toggle_selected")
			}
			NavigationAction::OpenSelected => {
				self.open_selected();
				Transition::changed("navigation.open_selected")
			}
			NavigationAction::CloseSelected => {
				let before = self.last_notice.clone();
				self.close_selected();
				if self.last_notice == before {
					Transition::unchanged("navigation.close_selected")
				} else {
					Transition::changed("navigation.close_selected")
				}
			}
		}
	}
}
