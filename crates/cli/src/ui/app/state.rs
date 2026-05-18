use std::collections::{BTreeMap, BTreeSet};

use code_moniker_core::lang::Lang;

use crate::ui::app::action::ShellAction;
use crate::ui::component::ComponentId;
use crate::ui::components::search_bar::{HeaderKindFilter, HeaderSearchState};
use crate::ui::contracts::Route;
use crate::ui::events::{FilterEdit, HeaderSearchFocus, Msg, UiMode};
use crate::ui::features::explorer::{
	ExplorerFeature, HeaderSearchResults, ROUTE_CHANGE, ROUTE_CHECK, ROUTE_OUTLINE, ROUTE_OVERVIEW,
	ROUTE_REFS,
};
use crate::ui::live::StoreEvent;
use crate::ui::reactive::Transition;
use crate::ui::runtime::{TaskId, TaskOutcome, TaskResult, WorkKind};
use crate::ui::store::navigation::{
	NavigationAction, NavigationPane, NavigationState, TreePaneAction,
};
use crate::workspace::{CheckSummary, UsageFocus};

use super::Effect;

const PANEL_SCROLL_STEP: usize = 8;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct WorkSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) epochs: BTreeMap<WorkKind, u64>,
	pub(in crate::ui) pending: BTreeSet<WorkKind>,
	pub(in crate::ui) running: BTreeMap<TaskId, RunningTask>,
}

impl WorkSlice {
	fn epoch(&self, work: WorkKind) -> u64 {
		self.epochs.get(&work).copied().unwrap_or(0)
	}

	fn bump_epoch(&mut self, work: WorkKind) {
		*self.epochs.entry(work).or_default() += 1;
	}

	fn bump_epochs(&mut self, works: &[WorkKind]) {
		self.generation += 1;
		for work in works {
			self.bump_epoch(*work);
		}
		self.pending.extend(works.iter().copied());
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct RunningTask {
	pub(in crate::ui) kind: WorkKind,
	pub(in crate::ui) generation: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum View {
	Overview,
	Tree,
	Refs,
	Check,
	Change,
}

impl View {
	pub(in crate::ui) fn route_path(self) -> &'static str {
		match self {
			Self::Overview => ROUTE_OVERVIEW,
			Self::Tree => ROUTE_OUTLINE,
			Self::Refs => ROUTE_REFS,
			Self::Check => ROUTE_CHECK,
			Self::Change => ROUTE_CHANGE,
		}
	}

	pub(in crate::ui) fn from_route_path(path: &str) -> Option<Self> {
		match path {
			ROUTE_OVERVIEW => Some(Self::Overview),
			ROUTE_OUTLINE => Some(Self::Tree),
			ROUTE_REFS => Some(Self::Refs),
			ROUTE_CHECK => Some(Self::Check),
			ROUTE_CHANGE => Some(Self::Change),
			_ => None,
		}
	}

	pub(in crate::ui) fn route(self) -> Route {
		ExplorerFeature::route(self.route_path())
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum VisualizationMode {
	Explorer,
	Search,
	Change,
}

impl VisualizationMode {
	pub(in crate::ui) fn label(self) -> &'static str {
		match self {
			Self::Explorer => "explorer",
			Self::Search => "search",
			Self::Change => "change",
		}
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum ChangePanelMode {
	Diff,
	Usages,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum PanelPolicy {
	Contextual,
	Manual,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) enum FocusRegion {
	#[default]
	Navigator,
	UsageLens,
	Panel,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct PanelNavigationState {
	pub(in crate::ui) component: Option<ComponentId>,
	pub(in crate::ui) selected: Option<usize>,
	pub(in crate::ui) scroll: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) enum ActiveFilter {
	#[default]
	None,
	HeaderSearch(HeaderSearchResults),
	Change,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ShellSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) status: String,
	pub(in crate::ui) route: Route,
	pub(in crate::ui) view: View,
	pub(in crate::ui) view_mode: VisualizationMode,
	pub(in crate::ui) panel_policy: PanelPolicy,
	pub(in crate::ui) change_panel: ChangePanelMode,
	pub(in crate::ui) mode: UiMode,
	pub(in crate::ui) focus_region: FocusRegion,
	pub(in crate::ui) active_filter: ActiveFilter,
	pub(in crate::ui) usage_lens: Option<UsageFocus>,
	pub(in crate::ui) header_search: HeaderSearchState,
	pub(in crate::ui) panel_navigation: PanelNavigationState,
}

impl Default for ShellSlice {
	fn default() -> Self {
		Self {
			generation: 0,
			status: String::new(),
			route: ExplorerFeature::route(ROUTE_OVERVIEW),
			view: View::Overview,
			view_mode: VisualizationMode::Explorer,
			panel_policy: PanelPolicy::Contextual,
			change_panel: ChangePanelMode::Diff,
			mode: UiMode::Normal,
			focus_region: FocusRegion::Navigator,
			active_filter: ActiveFilter::None,
			usage_lens: None,
			header_search: HeaderSearchState::default(),
			panel_navigation: PanelNavigationState::default(),
		}
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) enum CheckState {
	#[default]
	Pending,
	Ready(CheckSummary),
	Error(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct CheckSlice {
	pub(in crate::ui) state: CheckState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct NavigationSlice {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) state: Option<NavigationState>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct AppState {
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) shell: ShellSlice,
	pub(in crate::ui) check: CheckSlice,
	pub(in crate::ui) navigation: NavigationSlice,
	pub(in crate::ui) work: WorkSlice,
	pub(in crate::ui) last_task: Option<TaskSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct TaskSummary {
	pub(in crate::ui) id: TaskId,
	pub(in crate::ui) label: String,
	pub(in crate::ui) status: TaskStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum TaskStatus {
	Completed,
	Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum TaskCompletion {
	Accepted,
	Ignored,
}

impl TaskCompletion {
	pub(in crate::ui) fn accepted(self) -> bool {
		self == Self::Accepted
	}
}

impl AppState {
	pub(in crate::ui) fn new() -> Self {
		Self::default()
	}

	pub(in crate::ui) fn status(&self) -> &str {
		&self.shell.status
	}

	pub(in crate::ui) fn set_status(&mut self, status: impl Into<String>) {
		self.bump();
		self.shell.generation += 1;
		self.shell.status = status.into();
	}

	pub(in crate::ui) fn append_status(&mut self, suffix: impl AsRef<str>) {
		let suffix = suffix.as_ref();
		self.bump();
		self.shell.generation += 1;
		if self.shell.status.is_empty() {
			self.shell.status = suffix.to_string();
		} else {
			self.shell.status = format!("{}; {suffix}", self.shell.status);
		}
	}

	pub(in crate::ui) fn check_state(&self) -> &CheckState {
		&self.check.state
	}

	pub(in crate::ui) fn set_check_state(&mut self, state: CheckState) {
		self.bump();
		self.work.bump_epoch(WorkKind::CheckPanel);
		self.check.state = state;
	}

	pub(in crate::ui) fn set_navigation(&mut self, navigation: NavigationState) {
		self.bump();
		self.navigation.generation += 1;
		self.navigation.state = Some(navigation);
	}

	pub(in crate::ui) fn reduce_shell_action(&mut self, action: &ShellAction) -> Transition {
		match action {
			ShellAction::SetStatus(status) => {
				self.set_status(status.clone());
				Transition::changed()
			}
			ShellAction::AppendStatus(status) => {
				self.append_status(status);
				Transition::changed()
			}
			ShellAction::SetCheckState(state) => {
				self.set_check_state(state.clone());
				Transition::changed()
			}
			ShellAction::SetRoute(route) => {
				self.update_shell(|shell| shell.route = route.clone());
				Transition::changed()
			}
			ShellAction::SetView {
				view,
				policy,
				route,
			} => self.set_view_action(*view, *policy, route),
			ShellAction::ApplyHeaderSearch {
				results,
				return_focus,
			} => self.apply_header_search_action(results, *return_focus),
			ShellAction::SetHeaderSearchFilters {
				langs,
				kind_filters,
			} => self.set_header_search_filters_action(langs, kind_filters),
			ShellAction::SetHeaderSearchOptions {
				langs,
				kind_filters,
				available_langs,
				available_kind_filters,
				lang_cursor,
				kind_cursor,
			} => self.set_header_search_options_action(
				langs,
				kind_filters,
				available_langs,
				available_kind_filters,
				*lang_cursor,
				*kind_cursor,
			),
			ShellAction::SetHeaderSearchCursor { focus, cursor } => {
				self.set_header_search_cursor_action(*focus, *cursor)
			}
			ShellAction::ClearFilter { return_focus } => self.clear_filter_action(*return_focus),
			ShellAction::SetUsageLens(focus) => self.set_usage_lens_action(focus),
			ShellAction::EnterChangeMode => {
				self.update_shell(|shell| {
					shell.mode = UiMode::Normal;
					shell.focus_region = FocusRegion::Navigator;
					shell.active_filter = ActiveFilter::Change;
					shell.usage_lens = None;
					shell.view_mode = VisualizationMode::Change;
					shell.panel_policy = PanelPolicy::Contextual;
					shell.change_panel = ChangePanelMode::Diff;
					shell.panel_navigation = PanelNavigationState::default();
					shell.header_search.reset();
					shell.header_search.pending_generation = None;
				});
				Transition::changed()
			}
			ShellAction::ReplaceActiveFilter(active_filter) => {
				self.update_shell(|shell| shell.active_filter = active_filter.clone());
				Transition::changed()
			}
			ShellAction::SetChangePanel(change_panel) => {
				self.set_change_panel_action(*change_panel)
			}
			ShellAction::SetFocusRegion(region) => {
				self.update_shell(|shell| {
					shell.mode = UiMode::Normal;
					shell.focus_region = *region;
				});
				Transition::changed()
			}
			ShellAction::SetPanelScroll(offset) => {
				self.update_shell(|shell| shell.panel_navigation.scroll = *offset);
				Transition::changed()
			}
			ShellAction::SetPanelNavigation(state) => {
				self.update_shell(|shell| shell.panel_navigation = state.clone());
				Transition::changed()
			}
		}
	}

	fn set_view_action(&mut self, view: View, policy: PanelPolicy, route: &Route) -> Transition {
		self.update_shell(|shell| {
			if shell.view != view {
				shell.panel_navigation = PanelNavigationState::default();
			}
			shell.view = view;
			shell.panel_policy = policy;
			shell.route = route.clone();
		});
		Transition::changed()
	}

	fn clear_filter_action(&mut self, return_focus: bool) -> Transition {
		self.update_shell(|shell| {
			if return_focus {
				shell.mode = UiMode::Normal;
			}
			shell.focus_region = FocusRegion::Navigator;
			shell.active_filter = ActiveFilter::None;
			shell.usage_lens = None;
			shell.view_mode = VisualizationMode::Explorer;
			shell.panel_policy = PanelPolicy::Contextual;
			shell.change_panel = ChangePanelMode::Diff;
			shell.panel_navigation = PanelNavigationState::default();
			shell.header_search.reset();
			shell.header_search.pending_generation = None;
		});
		Transition::changed()
	}

	fn set_usage_lens_action(&mut self, focus: &Option<UsageFocus>) -> Transition {
		self.update_shell(|shell| {
			shell.mode = UiMode::Normal;
			shell.focus_region = if focus.is_some() {
				FocusRegion::UsageLens
			} else {
				FocusRegion::Navigator
			};
			shell.usage_lens = focus.clone();
			shell.panel_policy = PanelPolicy::Contextual;
			shell.panel_navigation = PanelNavigationState::default();
		});
		Transition::changed()
	}

	fn set_change_panel_action(&mut self, change_panel: ChangePanelMode) -> Transition {
		self.update_shell(|shell| {
			if shell.change_panel != change_panel {
				shell.panel_navigation = PanelNavigationState::default();
			}
			shell.change_panel = change_panel;
		});
		Transition::changed()
	}

	fn scroll_panel(&mut self, direction: i8) -> Transition {
		self.update_shell(|shell| {
			if direction > 0 {
				shell.panel_navigation.scroll = shell
					.panel_navigation
					.scroll
					.saturating_add(PANEL_SCROLL_STEP);
			} else {
				shell.panel_navigation.scroll = shell
					.panel_navigation
					.scroll
					.saturating_sub(PANEL_SCROLL_STEP);
			}
		});
		Transition::changed()
	}

	pub(in crate::ui) fn reduce_ui_msg(&mut self, msg: &Msg) -> Transition {
		match msg {
			Msg::Quit => Transition::unchanged().with_effect(Effect::Quit),
			Msg::ShowView(view) => {
				Transition::unchanged().with_effect(Effect::Navigate(view.route()))
			}
			Msg::ToggleHeaderSearch => self.toggle_header_search(),
			Msg::ToggleFocusRegion => emit_effect(Effect::ToggleFocusRegion),
			Msg::HeaderSearchNextField => {
				let focus = match self.shell.mode {
					UiMode::HeaderSearch(focus) => focus.next(),
					UiMode::Normal => HeaderSearchFocus::Text,
				};
				self.update_shell(|shell| {
					shell.header_search.focus = focus;
					shell.header_search.combo_open = false;
					shell.mode = UiMode::HeaderSearch(focus);
				});
				self.shell.status = match focus {
					HeaderSearchFocus::Text => "search text focused".to_string(),
					HeaderSearchFocus::Lang => "language selector focused".to_string(),
					HeaderSearchFocus::Kind => "kind selector focused".to_string(),
				};
				Transition::changed()
			}
			Msg::HeaderSearchInput(edit) => {
				let generation = self.edit_header_search_input(*edit);
				let text = display_filter_text(&self.shell.header_search.text);
				self.shell.status = format!("search draft: {text}");
				Transition::changed().with_effect(Effect::DebounceHeaderSearch(generation))
			}
			Msg::HeaderSearchSelectNext => {
				emit_effect(Effect::CycleHeaderSearchSelector { direction: 1 })
			}
			Msg::HeaderSearchSelectPrevious => {
				emit_effect(Effect::CycleHeaderSearchSelector { direction: -1 })
			}
			Msg::HeaderSearchToggleSelection => emit_effect(Effect::ToggleHeaderSearchSelection),
			Msg::HeaderSearchReset => {
				let return_focus = matches!(self.shell.mode, UiMode::Normal);
				self.reset_header_search();
				self.shell.status = "search filters reset".to_string();
				Transition::changed().with_effect(Effect::ApplyHeaderSearch {
					generation: None,
					return_focus,
				})
			}
			Msg::HeaderSearchApply => match self.shell.mode {
				UiMode::HeaderSearch(HeaderSearchFocus::Text) | UiMode::Normal => {
					emit_effect(Effect::ApplyHeaderSearch {
						generation: None,
						return_focus: true,
					})
				}
				UiMode::HeaderSearch(HeaderSearchFocus::Lang | HeaderSearchFocus::Kind)
					if self.shell.header_search.combo_open =>
				{
					self.update_shell(|shell| shell.header_search.combo_open = false);
					emit_effect(Effect::ApplyHeaderSearch {
						generation: None,
						return_focus: false,
					})
				}
				UiMode::HeaderSearch(HeaderSearchFocus::Lang | HeaderSearchFocus::Kind) => {
					self.update_shell(|shell| shell.header_search.combo_open = true);
					Transition::changed()
				}
			},
			Msg::Help => {
				self.set_status(
					"keys: s search focus, Tab next search field, x reset filters, Enter/right open, Esc/left close, PgUp/PgDn scroll panel, d changes, u usages, y copy panel, 1-5 panels, c check, q quit",
				);
				Transition::changed()
			}
			Msg::FocusUsages => emit_effect(Effect::FocusUsages),
			Msg::ToggleChangeMode => emit_effect(Effect::ToggleChangeMode),
			Msg::CopyPanelSnapshot => emit_effect(Effect::CopyPanelSnapshot),
			Msg::RunCheck => emit_effect(Effect::RunCheck),
			Msg::MoveDown => self.reduce_vertical_navigation(1),
			Msg::MoveUp => self.reduce_vertical_navigation(-1),
			Msg::Home => self.reduce_positional_navigation(true),
			Msg::End => self.reduce_positional_navigation(false),
			Msg::PanelScrollDown => self.scroll_panel(1),
			Msg::PanelScrollUp => self.scroll_panel(-1),
			Msg::ToggleNode | Msg::OpenNode if self.shell.focus_region == FocusRegion::Panel => {
				Transition::unchanged()
			}
			Msg::ToggleNode => emit_effect(Effect::ToggleSelectedNode),
			Msg::OpenNode => emit_effect(Effect::OpenSelectedNode),
			Msg::CloseNode if self.shell.focus_region == FocusRegion::UsageLens => {
				emit_effect(Effect::CloseNodeOrClearScope)
			}
			Msg::CloseNode if self.shell.focus_region == FocusRegion::Panel => {
				emit_effect(Effect::ToggleFocusRegion)
			}
			Msg::CloseNode => emit_effect(Effect::CloseNodeOrClearScope),
			Msg::Noop => Transition::unchanged(),
		}
	}

	fn toggle_header_search(&mut self) -> Transition {
		let next = match self.shell.mode {
			UiMode::Normal => UiMode::HeaderSearch(self.shell.header_search.focus),
			UiMode::HeaderSearch(_) => UiMode::Normal,
		};
		self.update_shell(|shell| {
			shell.mode = next;
			shell.header_search.combo_open = false;
			if matches!(next, UiMode::Normal) {
				shell.focus_region = FocusRegion::Navigator;
			}
		});
		self.shell.status = match next {
			UiMode::Normal => "search focus returned to navigator".to_string(),
			UiMode::HeaderSearch(HeaderSearchFocus::Text) => {
				"type to search; Tab selects lang".to_string()
			}
			UiMode::HeaderSearch(HeaderSearchFocus::Lang) => {
				"select language; Tab selects kind".to_string()
			}
			UiMode::HeaderSearch(HeaderSearchFocus::Kind) => {
				"select kind; Tab returns to text".to_string()
			}
		};
		Transition::changed()
	}

	fn reduce_vertical_navigation(&self, direction: i8) -> Transition {
		match self.shell.focus_region {
			FocusRegion::Navigator => {
				if direction > 0 {
					emit_effect(Effect::Navigation(Box::new(NavigationAction::Pane {
						pane: NavigationPane::Primary,
						action: TreePaneAction::MoveDown,
					})))
				} else {
					emit_effect(Effect::Navigation(Box::new(NavigationAction::Pane {
						pane: NavigationPane::Primary,
						action: TreePaneAction::MoveUp,
					})))
				}
			}
			FocusRegion::UsageLens => {
				if direction > 0 {
					emit_effect(Effect::Navigation(Box::new(NavigationAction::Pane {
						pane: NavigationPane::UsageLens,
						action: TreePaneAction::MoveDown,
					})))
				} else {
					emit_effect(Effect::Navigation(Box::new(NavigationAction::Pane {
						pane: NavigationPane::UsageLens,
						action: TreePaneAction::MoveUp,
					})))
				}
			}
			FocusRegion::Panel => emit_effect(Effect::PanelMove { direction }),
		}
	}

	fn reduce_positional_navigation(&self, home: bool) -> Transition {
		match self.shell.focus_region {
			FocusRegion::Navigator => {
				let action = if home {
					TreePaneAction::Home
				} else {
					TreePaneAction::End
				};
				emit_effect(Effect::Navigation(Box::new(NavigationAction::Pane {
					pane: NavigationPane::Primary,
					action,
				})))
			}
			FocusRegion::UsageLens => {
				let action = if home {
					TreePaneAction::Home
				} else {
					TreePaneAction::End
				};
				emit_effect(Effect::Navigation(Box::new(NavigationAction::Pane {
					pane: NavigationPane::UsageLens,
					action,
				})))
			}
			FocusRegion::Panel if home => emit_effect(Effect::PanelHome),
			FocusRegion::Panel => emit_effect(Effect::PanelEnd),
		}
	}

	pub(in crate::ui) fn reduce_header_search_debounced(&mut self, generation: u64) -> Transition {
		if self.shell.header_search.pending_generation == Some(generation) {
			emit_effect(Effect::ApplyHeaderSearch {
				generation: Some(generation),
				return_focus: false,
			})
		} else {
			Transition::unchanged()
		}
	}

	pub(in crate::ui) fn generation_for_work(&self, work: WorkKind) -> u64 {
		self.work.epoch(work)
	}

	pub(in crate::ui) fn start_task(&mut self, id: TaskId, kind: WorkKind, generation: u64) {
		self.bump();
		self.work.pending.remove(&kind);
		self.work
			.running
			.insert(id, RunningTask { kind, generation });
		if kind == WorkKind::CheckPanel {
			self.check.state = CheckState::Pending;
		}
	}

	pub(in crate::ui) fn invalidate_for_store_event(&mut self, event: StoreEvent) {
		self.bump();
		match event {
			StoreEvent::FullIndex => self.invalidate_full_index(),
			StoreEvent::GitOverlay => self.invalidate_git_overlay(),
		}
	}

	pub(in crate::ui) fn accepts_task_result(&self, result: &TaskResult) -> bool {
		self.work.running.get(&result.id).is_some_and(|running| {
			running.kind == result.work
				&& running.generation == result.generation
				&& self.generation_for_work(result.work) == result.generation
		})
	}

	pub(in crate::ui) fn complete_task(&mut self, result: &TaskResult) -> TaskCompletion {
		let accepted = self.accepts_task_result(result);
		self.bump();
		self.work.running.remove(&result.id);
		if !accepted {
			return TaskCompletion::Ignored;
		}
		match &result.outcome {
			TaskOutcome::StoreReloaded(_) => {
				self.work.pending.remove(&WorkKind::ProjectLoad);
				self.work.pending.remove(&WorkKind::FileCatalog);
				self.work.pending.remove(&WorkKind::GraphIndex);
				self.work.pending.remove(&WorkKind::SearchIndex);
				self.work.pending.remove(&WorkKind::GitOverlay);
				self.work.pending.remove(&WorkKind::ImpactIndex);
				self.work.pending.remove(&WorkKind::PanelData);
			}
			TaskOutcome::GitOverlayRefreshed(_) => {
				self.work.pending.remove(&WorkKind::GitOverlay);
				self.work.pending.remove(&WorkKind::ImpactIndex);
				self.work.pending.remove(&WorkKind::PanelData);
			}
			TaskOutcome::FileCatalogLoaded(_) => {
				self.work.pending.remove(&WorkKind::ProjectLoad);
				self.work.pending.remove(&WorkKind::FileCatalog);
			}
			TaskOutcome::CheckCompleted(summary) => {
				self.check.state = CheckState::Ready((**summary).clone());
				self.work.pending.remove(&WorkKind::CheckPanel);
			}
			TaskOutcome::Failed(error) => {
				self.mark_failed(result.work, error.clone());
			}
		}
		self.last_task = Some(TaskSummary {
			id: result.id,
			label: result.label.clone(),
			status: match &result.outcome {
				TaskOutcome::FileCatalogLoaded(_) => TaskStatus::Completed,
				TaskOutcome::StoreReloaded(_) => TaskStatus::Completed,
				TaskOutcome::GitOverlayRefreshed(_) => TaskStatus::Completed,
				TaskOutcome::CheckCompleted(_) => TaskStatus::Completed,
				TaskOutcome::Failed(_) => TaskStatus::Failed,
			},
		});
		TaskCompletion::Accepted
	}

	fn invalidate_full_index(&mut self) {
		self.check.state = CheckState::Pending;
		self.work.bump_epochs(&[
			WorkKind::ProjectLoad,
			WorkKind::FileCatalog,
			WorkKind::GraphIndex,
			WorkKind::SearchIndex,
			WorkKind::GitOverlay,
			WorkKind::ImpactIndex,
			WorkKind::PanelData,
			WorkKind::CheckPanel,
			WorkKind::CoverageIndex,
		]);
	}

	fn invalidate_git_overlay(&mut self) {
		self.work.bump_epochs(&[
			WorkKind::GitOverlay,
			WorkKind::ImpactIndex,
			WorkKind::PanelData,
		]);
	}

	fn bump(&mut self) {
		self.generation += 1;
	}

	fn mark_failed(&mut self, kind: WorkKind, error: String) {
		if kind == WorkKind::CheckPanel {
			self.check.state = CheckState::Error(error);
		}
	}

	fn update_shell(&mut self, update: impl FnOnce(&mut ShellSlice)) {
		self.bump();
		self.shell.generation += 1;
		update(&mut self.shell);
	}

	fn apply_header_search_action(
		&mut self,
		results: &HeaderSearchResults,
		return_focus: bool,
	) -> Transition {
		self.update_shell(|shell| {
			if return_focus {
				shell.mode = UiMode::Normal;
				shell.header_search.combo_open = false;
				shell.focus_region = FocusRegion::Navigator;
			}
			shell.active_filter = ActiveFilter::HeaderSearch(results.clone());
			shell.view_mode = VisualizationMode::Search;
			shell.panel_policy = PanelPolicy::Contextual;
			shell.panel_navigation = PanelNavigationState::default();
			shell.header_search.text = results.text.clone();
			shell.header_search.langs = results.langs.clone();
			shell.header_search.kind_filters = results.kind_filters.clone();
			shell.header_search.pending_generation = None;
		});
		Transition::changed()
	}

	fn set_header_search_filters_action(
		&mut self,
		langs: &[Lang],
		kind_filters: &[HeaderKindFilter],
	) -> Transition {
		let mut generation = self.shell.header_search.generation;
		self.update_shell(|shell| {
			shell.header_search.langs = langs.to_vec();
			shell.header_search.kind_filters = kind_filters.to_vec();
			generation = shell.header_search.bump_pending();
		});
		Transition::changed().with_effect(Effect::DebounceHeaderSearch(generation))
	}

	fn set_header_search_options_action(
		&mut self,
		langs: &[Lang],
		kind_filters: &[HeaderKindFilter],
		available_langs: &[Lang],
		available_kind_filters: &[HeaderKindFilter],
		lang_cursor: usize,
		kind_cursor: usize,
	) -> Transition {
		self.update_shell(|shell| {
			shell.header_search.langs = langs.to_vec();
			shell.header_search.kind_filters = kind_filters.to_vec();
			shell.header_search.available_langs = available_langs.to_vec();
			shell.header_search.available_kind_filters = available_kind_filters.to_vec();
			shell.header_search.lang_cursor = lang_cursor;
			shell.header_search.kind_cursor = kind_cursor;
		});
		Transition::changed()
	}

	fn set_header_search_cursor_action(
		&mut self,
		focus: HeaderSearchFocus,
		cursor: usize,
	) -> Transition {
		self.update_shell(|shell| match focus {
			HeaderSearchFocus::Text => {}
			HeaderSearchFocus::Lang => shell.header_search.lang_cursor = cursor,
			HeaderSearchFocus::Kind => shell.header_search.kind_cursor = cursor,
		});
		Transition::changed()
	}

	fn edit_header_search_input(&mut self, edit: FilterEdit) -> u64 {
		let mut generation = self.shell.header_search.generation;
		self.update_shell(|shell| {
			match edit {
				FilterEdit::Push(c) => shell.header_search.text.push(c),
				FilterEdit::Backspace => {
					shell.header_search.text.pop();
				}
				FilterEdit::Clear => shell.header_search.text.clear(),
			}
			generation = shell.header_search.bump_pending();
		});
		generation
	}

	fn reset_header_search(&mut self) {
		self.update_shell(|shell| {
			shell.header_search.reset();
			shell.header_search.bump_pending();
		});
	}
}

fn display_filter_text(filter: &str) -> &str {
	if filter.is_empty() { "<empty>" } else { filter }
}

fn emit_effect(effect: Effect) -> Transition {
	Transition::unchanged().with_effect(effect)
}
