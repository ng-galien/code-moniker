use std::collections::BTreeSet;

use crate::ui::components::tree_pane::TreePaneState;
use crate::ui::navigator::{
	NavNode, NavNodeKind, NavRow, all_expanded_keys, filtered_expanded_keys, flatten_nav,
};
use crate::ui::reactive::{Reduce, Transition};
use crate::workspace::DefLocation;

use super::ids::NodeId;

pub(in crate::ui) use crate::ui::components::tree_pane::{
	TreePaneAction, TreePaneNotice as NavigationNotice,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum NavigationScope {
	Explorer,
	Filtered,
	Change,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum NavigationPane {
	Primary,
	UsageLens,
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
	SetUsageLens {
		visible_defs: Vec<DefLocation>,
		reset_expansion: bool,
		expand_symbols: bool,
	},
	ClearUsageLens,
	Select {
		pane: NavigationPane,
		target: NavigationSelection,
	},
	Pane {
		pane: NavigationPane,
		action: TreePaneAction,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum NavigationSelection {
	Def(DefLocation),
	FirstChange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct NavigationState {
	explorer: NavNode,
	change: NavNode,
	explorer_pane: TreePaneState,
	scoped_pane: TreePaneState,
	visible_defs: Vec<DefLocation>,
	scope: NavigationScope,
	last_notice: NavigationNotice,
	usage_lens: Option<UsageLensNavigationState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UsageLensNavigationState {
	pane: TreePaneState,
	visible_defs: Vec<DefLocation>,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct NavigationPaneView<'a> {
	pub(in crate::ui) rows: &'a [NavRow],
	pub(in crate::ui) selection: usize,
	pub(in crate::ui) expanded: &'a BTreeSet<NodeId>,
}

impl<'a> NavigationPaneView<'a> {
	fn from_pane(pane: &'a TreePaneState) -> Self {
		Self {
			rows: pane.rows(),
			selection: pane.selection(),
			expanded: pane.expanded(),
		}
	}

	pub(in crate::ui) fn selected_row(self) -> Option<&'a NavRow> {
		self.rows.get(self.selection)
	}

	pub(in crate::ui) fn selected_context(self) -> Option<NavigationSelectionView<'a>> {
		let row = self.selected_row()?;
		Some(NavigationSelectionView {
			row,
			expanded: self.expanded.contains(&row.key),
		})
	}
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct NavigationSelectionView<'a> {
	pub(in crate::ui) row: &'a NavRow,
	pub(in crate::ui) expanded: bool,
}

impl NavigationState {
	pub(in crate::ui) fn new(explorer: NavNode, change: NavNode) -> Self {
		let mut state = Self {
			explorer,
			change,
			explorer_pane: TreePaneState::new(),
			scoped_pane: TreePaneState::new(),
			visible_defs: Vec::new(),
			scope: NavigationScope::Explorer,
			last_notice: NavigationNotice::Noop,
			usage_lens: None,
		};
		state.refresh_primary_rows(None);
		state
	}

	pub(in crate::ui) fn primary_view(&self) -> NavigationPaneView<'_> {
		NavigationPaneView::from_pane(self.active_primary_pane())
	}

	pub(in crate::ui) fn visible_defs(&self) -> &[DefLocation] {
		&self.visible_defs
	}

	pub(in crate::ui) fn usage_view(&self) -> Option<NavigationPaneView<'_>> {
		self.usage_lens
			.as_ref()
			.map(|lens| NavigationPaneView::from_pane(&lens.pane))
	}

	pub(in crate::ui) fn pane_view(&self, pane: NavigationPane) -> Option<NavigationPaneView<'_>> {
		match pane {
			NavigationPane::Primary => Some(self.primary_view()),
			NavigationPane::UsageLens => self.usage_view(),
		}
	}

	pub(in crate::ui) fn explorer_def_count(&self) -> usize {
		self.explorer.def_count
	}

	pub(in crate::ui) fn last_notice(&self) -> &NavigationNotice {
		&self.last_notice
	}

	fn active_primary_pane(&self) -> &TreePaneState {
		if self.is_scoped() {
			&self.scoped_pane
		} else {
			&self.explorer_pane
		}
	}

	fn active_primary_pane_mut(&mut self) -> &mut TreePaneState {
		if self.is_scoped() {
			&mut self.scoped_pane
		} else {
			&mut self.explorer_pane
		}
	}

	fn is_scoped(&self) -> bool {
		matches!(
			self.scope,
			NavigationScope::Filtered | NavigationScope::Change
		)
	}

	fn replace_models(&mut self, explorer: NavNode, change: NavNode) {
		let primary_key = self.active_primary_pane().selected_key();
		let usage_key = self
			.usage_lens
			.as_ref()
			.and_then(|lens| lens.pane.selected_key());
		self.explorer = explorer;
		self.change = change;
		self.retain_valid_expansion();
		self.refresh_primary_rows(primary_key.as_ref());
		self.refresh_usage_rows(usage_key.as_ref());
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
		let selected_key = if reset_expansion {
			None
		} else {
			self.active_primary_pane().selected_key()
		};
		if reset_expansion {
			self.reset_active_scope(expand_symbols);
		}
		self.refresh_primary_rows(selected_key.as_ref());
	}

	fn reset_active_scope(&mut self, expand_symbols: bool) {
		self.active_primary_pane_mut().reset_selection();
		match self.scope {
			NavigationScope::Explorer => {}
			NavigationScope::Filtered => {
				self.scoped_pane.set_expanded(filtered_expanded_keys(
					&self.explorer,
					&self.visible_defs,
					expand_symbols,
				));
			}
			NavigationScope::Change => {
				self.scoped_pane
					.set_expanded(all_expanded_keys(&self.change));
			}
		}
	}

	fn set_usage_lens(
		&mut self,
		visible_defs: Vec<DefLocation>,
		reset_expansion: bool,
		expand_symbols: bool,
	) {
		let mut pane = if reset_expansion {
			TreePaneState::new()
		} else {
			self.usage_lens
				.as_ref()
				.map(|lens| lens.pane.clone())
				.unwrap_or_else(TreePaneState::new)
		};
		let selected_key = if reset_expansion {
			None
		} else {
			pane.selected_key()
		};
		if reset_expansion {
			pane.set_expanded(filtered_expanded_keys(
				&self.explorer,
				&visible_defs,
				expand_symbols,
			));
			pane.reset_selection();
		}
		self.usage_lens = Some(UsageLensNavigationState { pane, visible_defs });
		self.refresh_usage_rows(selected_key.as_ref());
	}

	fn clear_usage_lens(&mut self) {
		self.usage_lens = None;
	}

	fn apply_pane_action(&mut self, pane: NavigationPane, action: TreePaneAction) -> Transition {
		let before = self.pane_selection(pane);
		let notice = match pane {
			NavigationPane::Primary => self.apply_primary_pane_action(action),
			NavigationPane::UsageLens => self.apply_usage_pane_action(action),
		};
		self.last_notice = notice;
		let changed = self.pane_selection(pane) != before
			|| !matches!(self.last_notice, NavigationNotice::Noop);
		if changed {
			Transition::changed()
		} else {
			Transition::unchanged()
		}
	}

	fn apply_selection(&mut self, pane: NavigationPane, target: NavigationSelection) -> Transition {
		let before = self.pane_selection(pane);
		match pane {
			NavigationPane::Primary => select_in_pane(self.active_primary_pane_mut(), target),
			NavigationPane::UsageLens => {
				if let Some(lens) = &mut self.usage_lens {
					select_in_pane(&mut lens.pane, target);
				}
			}
		}
		if self.pane_selection(pane) == before {
			Transition::unchanged()
		} else {
			Transition::changed()
		}
	}

	fn apply_primary_pane_action(&mut self, action: TreePaneAction) -> NavigationNotice {
		let notice = self.active_primary_pane_mut().apply(action);
		let selected_key = self.active_primary_pane().selected_key();
		self.refresh_primary_rows(selected_key.as_ref());
		notice
	}

	fn apply_usage_pane_action(&mut self, action: TreePaneAction) -> NavigationNotice {
		let Some(lens) = &mut self.usage_lens else {
			return NavigationNotice::Noop;
		};
		let notice = lens.pane.apply(action);
		let selected_key = lens.pane.selected_key();
		self.refresh_usage_rows(selected_key.as_ref());
		notice
	}

	fn pane_selection(&self, pane: NavigationPane) -> usize {
		match pane {
			NavigationPane::Primary => self.primary_view().selection,
			NavigationPane::UsageLens => self.usage_view().map_or(0, |view| view.selection),
		}
	}

	fn refresh_primary_rows(&mut self, selected_key: Option<&NodeId>) {
		let rows = match self.scope {
			NavigationScope::Explorer => {
				rows_for(&self.explorer, self.explorer_pane.expanded(), None)
			}
			NavigationScope::Filtered => rows_for(
				&self.explorer,
				self.scoped_pane.expanded(),
				Some(self.visible_defs.as_slice()),
			),
			NavigationScope::Change => rows_for(&self.change, self.scoped_pane.expanded(), None),
		};
		self.active_primary_pane_mut()
			.replace_rows(rows, selected_key);
	}

	fn refresh_usage_rows(&mut self, selected_key: Option<&NodeId>) {
		let Some(lens) = &self.usage_lens else {
			return;
		};
		let rows = rows_for(
			&self.explorer,
			lens.pane.expanded(),
			Some(lens.visible_defs.as_slice()),
		);
		if let Some(lens) = &mut self.usage_lens {
			lens.pane.replace_rows(rows, selected_key);
		}
	}

	fn retain_valid_expansion(&mut self) {
		let valid_explorer = all_expanded_keys(&self.explorer);
		self.explorer_pane.retain_expanded(&valid_explorer);
		self.scoped_pane
			.retain_expanded(&valid_for_scoped(&self.explorer, &self.change));
		if let Some(lens) = &mut self.usage_lens {
			lens.pane.retain_expanded(&valid_explorer);
		}
	}
}

fn rows_for(
	model: &NavNode,
	expanded: &BTreeSet<NodeId>,
	matches: Option<&[DefLocation]>,
) -> Vec<NavRow> {
	let mut rows = Vec::new();
	flatten_nav(model, expanded, matches, 0, &mut rows);
	rows
}

fn valid_for_scoped(explorer: &NavNode, change: &NavNode) -> BTreeSet<NodeId> {
	let mut valid = all_expanded_keys(explorer);
	valid.extend(all_expanded_keys(change));
	valid
}

impl Reduce<NavigationAction> for NavigationState {
	fn reduce(&mut self, action: NavigationAction) -> Transition {
		self.last_notice = NavigationNotice::Noop;
		match action {
			NavigationAction::ReplaceModels { explorer, change } => {
				self.replace_models(explorer, change);
				Transition::changed()
			}
			NavigationAction::SetScope {
				scope,
				visible_defs,
				reset_expansion,
				expand_symbols,
			} => {
				self.set_scope(scope, visible_defs, reset_expansion, expand_symbols);
				Transition::changed()
			}
			NavigationAction::SetUsageLens {
				visible_defs,
				reset_expansion,
				expand_symbols,
			} => {
				self.set_usage_lens(visible_defs, reset_expansion, expand_symbols);
				Transition::changed()
			}
			NavigationAction::ClearUsageLens => {
				self.clear_usage_lens();
				Transition::changed()
			}
			NavigationAction::Select { pane, target } => self.apply_selection(pane, target),
			NavigationAction::Pane { pane, action } => self.apply_pane_action(pane, action),
		}
	}
}

fn select_in_pane(pane: &mut TreePaneState, target: NavigationSelection) {
	match target {
		NavigationSelection::Def(loc) => {
			pane.select_first_matching(
				|row| matches!(row.kind, NavNodeKind::Def(row_loc) if row_loc == loc),
			);
		}
		NavigationSelection::FirstChange => {
			pane.select_first_matching(|row| matches!(row.kind, NavNodeKind::Change(_)));
		}
	}
}
