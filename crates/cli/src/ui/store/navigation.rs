use std::collections::BTreeSet;

use crate::ui::store::reducer::{Reduce, Transition};
use crate::ui::store::tree_pane_state::{
	TreePaneState, tree_pane_apply, tree_pane_expanded, tree_pane_replace_rows,
	tree_pane_reset_selection, tree_pane_retain_expanded, tree_pane_rows,
	tree_pane_select_first_matching, tree_pane_selected_key, tree_pane_selection,
	tree_pane_set_expanded,
};
use code_moniker_workspace::snapshot::SymbolId;
type DefLocation = SymbolId;

use super::ids::NodeId;
use super::navigation_tree::{
	NavNode, NavNodeKind, NavRow, all_expanded_keys, filtered_expanded_keys, flatten_nav,
};
use super::tree_pane_action::{TreePaneAction, TreePaneNotice};

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

#[allow(clippy::large_enum_variant)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
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
	last_notice: TreePaneNotice,
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
			rows: tree_pane_rows(pane),
			selection: tree_pane_selection(pane),
			expanded: tree_pane_expanded(pane),
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
		new_navigation_state(explorer, change)
	}
}

pub(in crate::ui) fn navigation_primary_view(state: &NavigationState) -> NavigationPaneView<'_> {
	NavigationPaneView::from_pane(active_primary_pane(state))
}

pub(in crate::ui) fn navigation_visible_defs(state: &NavigationState) -> &[DefLocation] {
	&state.visible_defs
}

pub(in crate::ui) fn navigation_usage_view(
	state: &NavigationState,
) -> Option<NavigationPaneView<'_>> {
	state
		.usage_lens
		.as_ref()
		.map(|lens| NavigationPaneView::from_pane(&lens.pane))
}

pub(in crate::ui) fn navigation_pane_view(
	state: &NavigationState,
	pane: NavigationPane,
) -> Option<NavigationPaneView<'_>> {
	match pane {
		NavigationPane::Primary => Some(navigation_primary_view(state)),
		NavigationPane::UsageLens => navigation_usage_view(state),
	}
}

pub(in crate::ui) fn navigation_explorer_def_count(state: &NavigationState) -> usize {
	state.explorer.def_count
}

pub(in crate::ui) fn navigation_last_notice(state: &NavigationState) -> &TreePaneNotice {
	&state.last_notice
}

fn new_navigation_state(explorer: NavNode, change: NavNode) -> NavigationState {
	let mut state = NavigationState {
		explorer,
		change,
		explorer_pane: TreePaneState::new(),
		scoped_pane: TreePaneState::new(),
		visible_defs: Vec::new(),
		scope: NavigationScope::Explorer,
		last_notice: TreePaneNotice::Noop,
		usage_lens: None,
	};
	refresh_primary_rows(&mut state, None);
	state
}

fn active_primary_pane(state: &NavigationState) -> &TreePaneState {
	if is_scoped(state) {
		&state.scoped_pane
	} else {
		&state.explorer_pane
	}
}

fn active_primary_pane_mut(state: &mut NavigationState) -> &mut TreePaneState {
	if is_scoped(state) {
		&mut state.scoped_pane
	} else {
		&mut state.explorer_pane
	}
}

fn is_scoped(state: &NavigationState) -> bool {
	matches!(
		state.scope,
		NavigationScope::Filtered | NavigationScope::Change
	)
}

fn replace_models(state: &mut NavigationState, explorer: NavNode, change: NavNode) {
	let primary_key = tree_pane_selected_key(active_primary_pane(state));
	let usage_key = state
		.usage_lens
		.as_ref()
		.and_then(|lens| tree_pane_selected_key(&lens.pane));
	state.explorer = explorer;
	state.change = change;
	retain_valid_expansion(state);
	refresh_primary_rows(state, primary_key.as_ref());
	refresh_usage_rows(state, usage_key.as_ref());
}

fn set_scope(
	state: &mut NavigationState,
	scope: NavigationScope,
	visible_defs: Vec<DefLocation>,
	reset_expansion: bool,
	expand_symbols: bool,
) {
	state.scope = scope;
	state.visible_defs = visible_defs;
	let selected_key = if reset_expansion {
		None
	} else {
		tree_pane_selected_key(active_primary_pane(state))
	};
	if reset_expansion {
		reset_active_scope(state, expand_symbols);
	}
	refresh_primary_rows(state, selected_key.as_ref());
}

fn reset_active_scope(state: &mut NavigationState, expand_symbols: bool) {
	tree_pane_reset_selection(active_primary_pane_mut(state));
	match state.scope {
		NavigationScope::Explorer => {}
		NavigationScope::Filtered => {
			tree_pane_set_expanded(
				&mut state.scoped_pane,
				filtered_expanded_keys(&state.explorer, &state.visible_defs, expand_symbols),
			);
		}
		NavigationScope::Change => {
			tree_pane_set_expanded(&mut state.scoped_pane, all_expanded_keys(&state.change));
		}
	}
}

fn set_usage_lens(
	state: &mut NavigationState,
	visible_defs: Vec<DefLocation>,
	reset_expansion: bool,
	expand_symbols: bool,
) {
	let mut pane = if reset_expansion {
		TreePaneState::new()
	} else {
		state
			.usage_lens
			.as_ref()
			.map(|lens| lens.pane.clone())
			.unwrap_or_else(TreePaneState::new)
	};
	let selected_key = if reset_expansion {
		None
	} else {
		tree_pane_selected_key(&pane)
	};
	if reset_expansion {
		tree_pane_set_expanded(
			&mut pane,
			filtered_expanded_keys(&state.explorer, &visible_defs, expand_symbols),
		);
		tree_pane_reset_selection(&mut pane);
	}
	state.usage_lens = Some(UsageLensNavigationState { pane, visible_defs });
	refresh_usage_rows(state, selected_key.as_ref());
}

fn clear_usage_lens(state: &mut NavigationState) {
	state.usage_lens = None;
}

fn apply_pane_action(
	state: &mut NavigationState,
	pane: NavigationPane,
	action: TreePaneAction,
) -> Transition {
	let before = pane_selection(state, pane);
	let notice = match pane {
		NavigationPane::Primary => apply_primary_pane_action(state, action),
		NavigationPane::UsageLens => apply_usage_pane_action(state, action),
	};
	state.last_notice = notice;
	let changed =
		pane_selection(state, pane) != before || !matches!(state.last_notice, TreePaneNotice::Noop);
	if changed {
		Transition::changed()
	} else {
		Transition::unchanged()
	}
}

fn apply_selection(
	state: &mut NavigationState,
	pane: NavigationPane,
	target: NavigationSelection,
) -> Transition {
	let before = pane_selection(state, pane);
	match pane {
		NavigationPane::Primary => select_in_pane(active_primary_pane_mut(state), target),
		NavigationPane::UsageLens => {
			if let Some(lens) = &mut state.usage_lens {
				select_in_pane(&mut lens.pane, target);
			}
		}
	}
	if pane_selection(state, pane) == before {
		Transition::unchanged()
	} else {
		Transition::changed()
	}
}

fn apply_primary_pane_action(
	state: &mut NavigationState,
	action: TreePaneAction,
) -> TreePaneNotice {
	let notice = tree_pane_apply(active_primary_pane_mut(state), action);
	if notice_changes_rows(&notice) {
		let selected_key = tree_pane_selected_key(active_primary_pane(state));
		refresh_primary_rows(state, selected_key.as_ref());
	}
	notice
}

fn apply_usage_pane_action(state: &mut NavigationState, action: TreePaneAction) -> TreePaneNotice {
	let Some(lens) = &mut state.usage_lens else {
		return TreePaneNotice::Noop;
	};
	let notice = tree_pane_apply(&mut lens.pane, action);
	if notice_changes_rows(&notice) {
		let selected_key = tree_pane_selected_key(&lens.pane);
		refresh_usage_rows(state, selected_key.as_ref());
	}
	notice
}

fn notice_changes_rows(notice: &TreePaneNotice) -> bool {
	matches!(
		notice,
		TreePaneNotice::Opened(_) | TreePaneNotice::Closed(_)
	)
}

fn pane_selection(state: &NavigationState, pane: NavigationPane) -> usize {
	match pane {
		NavigationPane::Primary => navigation_primary_view(state).selection,
		NavigationPane::UsageLens => navigation_usage_view(state).map_or(0, |view| view.selection),
	}
}

fn refresh_primary_rows(state: &mut NavigationState, selected_key: Option<&NodeId>) {
	let rows = match state.scope {
		NavigationScope::Explorer => rows_for(
			&state.explorer,
			tree_pane_expanded(&state.explorer_pane),
			None,
		),
		NavigationScope::Filtered => rows_for(
			&state.explorer,
			tree_pane_expanded(&state.scoped_pane),
			Some(state.visible_defs.as_slice()),
		),
		NavigationScope::Change => {
			rows_for(&state.change, tree_pane_expanded(&state.scoped_pane), None)
		}
	};
	tree_pane_replace_rows(active_primary_pane_mut(state), rows, selected_key);
}

fn refresh_usage_rows(state: &mut NavigationState, selected_key: Option<&NodeId>) {
	let Some(lens) = &state.usage_lens else {
		return;
	};
	let rows = rows_for(
		&state.explorer,
		tree_pane_expanded(&lens.pane),
		Some(lens.visible_defs.as_slice()),
	);
	if let Some(lens) = &mut state.usage_lens {
		tree_pane_replace_rows(&mut lens.pane, rows, selected_key);
	}
}

fn retain_valid_expansion(state: &mut NavigationState) {
	let valid_explorer = all_expanded_keys(&state.explorer);
	tree_pane_retain_expanded(&mut state.explorer_pane, &valid_explorer);
	tree_pane_retain_expanded(
		&mut state.scoped_pane,
		&valid_for_scoped(&state.explorer, &state.change),
	);
	if let Some(lens) = &mut state.usage_lens {
		tree_pane_retain_expanded(&mut lens.pane, &valid_explorer);
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
		self.last_notice = TreePaneNotice::Noop;
		match action {
			NavigationAction::ReplaceModels { explorer, change } => {
				replace_models(self, explorer, change);
				Transition::changed()
			}
			NavigationAction::SetScope {
				scope,
				visible_defs,
				reset_expansion,
				expand_symbols,
			} => {
				set_scope(self, scope, visible_defs, reset_expansion, expand_symbols);
				Transition::changed()
			}
			NavigationAction::SetUsageLens {
				visible_defs,
				reset_expansion,
				expand_symbols,
			} => {
				set_usage_lens(self, visible_defs, reset_expansion, expand_symbols);
				Transition::changed()
			}
			NavigationAction::ClearUsageLens => {
				clear_usage_lens(self);
				Transition::changed()
			}
			NavigationAction::Select { pane, target } => apply_selection(self, pane, target),
			NavigationAction::Pane { pane, action } => apply_pane_action(self, pane, action),
		}
	}
}

fn select_in_pane(pane: &mut TreePaneState, target: NavigationSelection) {
	match target {
		NavigationSelection::Def(loc) => {
			tree_pane_select_first_matching(
				pane,
				|row| matches!(&row.kind, NavNodeKind::Def(row_loc) if row_loc == &loc),
			);
		}
		NavigationSelection::FirstChange => {
			tree_pane_select_first_matching(pane, |row| matches!(row.kind, NavNodeKind::Change(_)));
		}
	}
}
