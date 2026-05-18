use std::collections::BTreeSet;

use crate::ui::navigator::{
	NavNode, NavNodeKind, NavRow, all_expanded_keys, filtered_expanded_keys, flatten_nav,
};
use crate::ui::reactive::{Reduce, Transition};
use crate::workspace::DefLocation;

use super::ids::NodeId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum NavigationScope {
	Explorer,
	Filtered,
	Change,
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
	SetUsageLens {
		visible_defs: Vec<DefLocation>,
		reset_expansion: bool,
		expand_symbols: bool,
	},
	ClearUsageLens,
	UsageMoveDown,
	UsageMoveUp,
	UsageHome,
	UsageEnd,
	UsageToggleSelected,
	UsageOpenSelected,
	UsageCloseSelected,
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
	usage_lens: Option<UsageLensNavigationState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UsageLensNavigationState {
	expanded: BTreeSet<NodeId>,
	rows: Vec<NavRow>,
	selection: usize,
	visible_defs: Vec<DefLocation>,
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
			usage_lens: None,
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

	pub(in crate::ui) fn usage_rows(&self) -> &[NavRow] {
		self.usage_lens
			.as_ref()
			.map_or(&[], |lens| lens.rows.as_slice())
	}

	pub(in crate::ui) fn usage_selected_row(&self) -> Option<&NavRow> {
		self.usage_lens
			.as_ref()
			.and_then(|lens| lens.rows.get(lens.selection))
	}

	pub(in crate::ui) fn usage_selection(&self) -> usize {
		self.usage_lens.as_ref().map_or(0, |lens| lens.selection)
	}

	pub(in crate::ui) fn usage_expanded(&self) -> Option<&BTreeSet<NodeId>> {
		self.usage_lens.as_ref().map(|lens| &lens.expanded)
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
			NavigationScope::Filtered | NavigationScope::Change
		)
	}

	fn replace_models(&mut self, explorer: NavNode, change: NavNode) {
		let selected_key = self.selected_row().map(|row| row.key.clone());
		let usage_selected_key = self.usage_selected_row().map(|row| row.key.clone());
		self.explorer = explorer;
		self.change = change;
		self.retain_valid_expansion();
		self.refresh_rows();
		self.restore_selection(selected_key.as_ref());
		self.refresh_usage_rows();
		self.restore_usage_selection(usage_selected_key.as_ref());
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
				NavigationScope::Explorer => {}
			}
			self.selection = 0;
		}
		self.refresh_rows();
	}

	fn refresh_rows(&mut self) {
		self.rows.clear();
		match self.scope {
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

	fn refresh_usage_rows(&mut self) {
		let Some(lens) = &mut self.usage_lens else {
			return;
		};
		lens.rows.clear();
		flatten_nav(
			&self.explorer,
			&lens.expanded,
			Some(lens.visible_defs.as_slice()),
			0,
			&mut lens.rows,
		);
		clamp_index(&mut lens.selection, lens.rows.len());
	}

	fn retain_valid_expansion(&mut self) {
		let valid_explorer = all_expanded_keys(&self.explorer);
		self.expanded.retain(|key| valid_explorer.contains(key));
		let mut valid_filtered = valid_explorer;
		valid_filtered.extend(all_expanded_keys(&self.change));
		self.filtered_expanded
			.retain(|key| valid_filtered.contains(key));
		if let Some(lens) = &mut self.usage_lens {
			lens.expanded.retain(|key| valid_filtered.contains(key));
		}
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

	fn set_usage_lens(
		&mut self,
		visible_defs: Vec<DefLocation>,
		reset_expansion: bool,
		expand_symbols: bool,
	) {
		let expanded = if !reset_expansion {
			self.usage_lens
				.as_ref()
				.map(|lens| lens.expanded.clone())
				.unwrap_or_default()
		} else {
			filtered_expanded_keys(&self.explorer, &visible_defs, expand_symbols)
		};
		let selection = if reset_expansion {
			0
		} else {
			self.usage_lens.as_ref().map_or(0, |lens| lens.selection)
		};
		self.usage_lens = Some(UsageLensNavigationState {
			expanded,
			rows: Vec::new(),
			selection,
			visible_defs,
		});
		self.refresh_usage_rows();
	}

	fn clear_usage_lens(&mut self) {
		self.usage_lens = None;
	}

	fn restore_usage_selection(&mut self, key: Option<&NodeId>) {
		let Some(lens) = &mut self.usage_lens else {
			return;
		};
		let Some(key) = key else {
			clamp_index(&mut lens.selection, lens.rows.len());
			return;
		};
		if let Some(idx) = lens.rows.iter().position(|row| &row.key == key) {
			lens.selection = idx;
		} else {
			clamp_index(&mut lens.selection, lens.rows.len());
		}
	}

	fn move_usage_down(&mut self) {
		let Some(lens) = &mut self.usage_lens else {
			return;
		};
		if lens.selection + 1 < lens.rows.len() {
			lens.selection += 1;
		}
	}

	fn move_usage_up(&mut self) {
		if let Some(lens) = &mut self.usage_lens {
			lens.selection = lens.selection.saturating_sub(1);
		}
	}

	fn toggle_usage_selected(&mut self) {
		let Some(lens) = &mut self.usage_lens else {
			return;
		};
		let Some(row) = lens.rows.get(lens.selection).cloned() else {
			return;
		};
		if !row.has_children {
			return;
		}
		if lens.expanded.remove(&row.key) {
			self.last_notice = NavigationNotice::Closed(row.label);
		} else {
			lens.expanded.insert(row.key);
			self.last_notice = NavigationNotice::Opened(row.label);
		}
		self.refresh_usage_rows();
	}

	fn open_usage_selected(&mut self) {
		let Some(lens) = &mut self.usage_lens else {
			return;
		};
		let Some(row) = lens.rows.get(lens.selection).cloned() else {
			return;
		};
		if row.has_children && !lens.expanded.contains(&row.key) {
			lens.expanded.insert(row.key);
			self.last_notice = NavigationNotice::Opened(row.label);
			self.refresh_usage_rows();
		}
	}

	fn close_usage_selected(&mut self) {
		let Some(lens) = &mut self.usage_lens else {
			return;
		};
		let Some(row) = lens.rows.get(lens.selection).cloned() else {
			return;
		};
		if row.has_children && lens.expanded.contains(&row.key) {
			lens.expanded.remove(&row.key);
			self.last_notice = NavigationNotice::Closed(row.label);
			self.refresh_usage_rows();
			return;
		}
		if row.depth == 0 {
			return;
		}
		let parent_depth = row.depth - 1;
		if let Some(parent) = lens.rows[..lens.selection]
			.iter()
			.rposition(|candidate| candidate.depth == parent_depth)
		{
			lens.selection = parent;
			self.last_notice = NavigationNotice::MovedToParent;
		}
	}
}

fn clamp_index(selection: &mut usize, len: usize) {
	if len == 0 {
		*selection = 0;
	} else if *selection >= len {
		*selection = len - 1;
	}
}

fn reduce_primary_navigation(
	state: &mut NavigationState,
	action: NavigationAction,
) -> Option<Transition> {
	match action {
		NavigationAction::ReplaceModels { explorer, change } => {
			state.replace_models(explorer, change);
			Some(Transition::changed("navigation.replace_models"))
		}
		NavigationAction::SetScope {
			scope,
			visible_defs,
			reset_expansion,
			expand_symbols,
		} => {
			state.set_scope(scope, visible_defs, reset_expansion, expand_symbols);
			Some(Transition::changed("navigation.set_scope"))
		}
		NavigationAction::MoveDown => {
			let before = state.selection;
			state.move_down();
			Some(selection_transition(
				state.selection,
				before,
				"navigation.move_down",
			))
		}
		NavigationAction::MoveUp => {
			let before = state.selection;
			state.move_up();
			Some(selection_transition(
				state.selection,
				before,
				"navigation.move_up",
			))
		}
		NavigationAction::Home => {
			let before = state.selection;
			state.selection = 0;
			Some(selection_transition(
				state.selection,
				before,
				"navigation.home",
			))
		}
		NavigationAction::End => {
			let before = state.selection;
			state.selection = state.rows.len().saturating_sub(1);
			Some(selection_transition(
				state.selection,
				before,
				"navigation.end",
			))
		}
		NavigationAction::SelectDef(loc) => {
			let before = state.selection;
			state.select_def(loc);
			Some(selection_transition(
				state.selection,
				before,
				"navigation.select_def",
			))
		}
		NavigationAction::SelectFirstChange => {
			let before = state.selection;
			state.select_first_change();
			Some(selection_transition(
				state.selection,
				before,
				"navigation.select_first_change",
			))
		}
		NavigationAction::ToggleSelected => {
			state.toggle_selected();
			Some(Transition::changed("navigation.toggle_selected"))
		}
		NavigationAction::OpenSelected => {
			state.open_selected();
			Some(Transition::changed("navigation.open_selected"))
		}
		NavigationAction::CloseSelected => {
			let before = state.last_notice.clone();
			state.close_selected();
			Some(notice_transition(
				&state.last_notice,
				&before,
				"navigation.close_selected",
			))
		}
		_ => None,
	}
}

fn reduce_usage_navigation(
	state: &mut NavigationState,
	action: NavigationAction,
) -> Option<Transition> {
	match action {
		NavigationAction::SetUsageLens {
			visible_defs,
			reset_expansion,
			expand_symbols,
		} => {
			state.set_usage_lens(visible_defs, reset_expansion, expand_symbols);
			Some(Transition::changed("navigation.set_usage_lens"))
		}
		NavigationAction::ClearUsageLens => {
			state.clear_usage_lens();
			Some(Transition::changed("navigation.clear_usage_lens"))
		}
		NavigationAction::UsageMoveDown => {
			let before = state.usage_selection();
			state.move_usage_down();
			Some(selection_transition(
				state.usage_selection(),
				before,
				"navigation.usage_move_down",
			))
		}
		NavigationAction::UsageMoveUp => {
			let before = state.usage_selection();
			state.move_usage_up();
			Some(selection_transition(
				state.usage_selection(),
				before,
				"navigation.usage_move_up",
			))
		}
		NavigationAction::UsageHome => {
			let before = state.usage_selection();
			if let Some(lens) = &mut state.usage_lens {
				lens.selection = 0;
			}
			Some(selection_transition(
				state.usage_selection(),
				before,
				"navigation.usage_home",
			))
		}
		NavigationAction::UsageEnd => {
			let before = state.usage_selection();
			if let Some(lens) = &mut state.usage_lens {
				lens.selection = lens.rows.len().saturating_sub(1);
			}
			Some(selection_transition(
				state.usage_selection(),
				before,
				"navigation.usage_end",
			))
		}
		NavigationAction::UsageToggleSelected => {
			state.toggle_usage_selected();
			Some(Transition::changed("navigation.usage_toggle_selected"))
		}
		NavigationAction::UsageOpenSelected => {
			state.open_usage_selected();
			Some(Transition::changed("navigation.usage_open_selected"))
		}
		NavigationAction::UsageCloseSelected => {
			let before = state.last_notice.clone();
			state.close_usage_selected();
			Some(notice_transition(
				&state.last_notice,
				&before,
				"navigation.usage_close_selected",
			))
		}
		_ => None,
	}
}

fn selection_transition(current: usize, before: usize, reason: &'static str) -> Transition {
	if current == before {
		Transition::unchanged(reason)
	} else {
		Transition::changed(reason)
	}
}

fn notice_transition(
	current: &NavigationNotice,
	before: &NavigationNotice,
	reason: &'static str,
) -> Transition {
	if current == before {
		Transition::unchanged(reason)
	} else {
		Transition::changed(reason)
	}
}

impl Reduce<NavigationAction> for NavigationState {
	fn reduce(&mut self, action: NavigationAction) -> Transition {
		self.last_notice = NavigationNotice::Noop;
		reduce_primary_navigation(self, action.clone())
			.or_else(|| reduce_usage_navigation(self, action))
			.unwrap_or_else(|| Transition::unchanged("navigation.unhandled"))
	}
}
