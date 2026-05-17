use std::collections::{BTreeMap, BTreeSet};

use crate::ui::app::action::ShellAction;
use crate::ui::app::command::AppCommand;
use crate::ui::contracts::Route;
use crate::ui::events::{FilterEdit, Msg, UiMode};
use crate::ui::features::explorer::{
	ExplorerFeature, ROUTE_CHANGE, ROUTE_CHECK, ROUTE_OUTLINE, ROUTE_OVERVIEW, ROUTE_REFS,
};
use crate::ui::live::StoreEvent;
use crate::ui::reactive::Transition;
use crate::ui::runtime::{TaskId, TaskOutcome, TaskResult, WorkKind};
use crate::ui::store::navigation::{NavigationAction, NavigationState};
use crate::workspace::{CheckSummary, SearchHit, SymbolFilter, UsageFocus};

use super::Effect;

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
	pub(in crate::ui) fn next(self) -> Self {
		match self {
			Self::Overview => Self::Tree,
			Self::Tree => Self::Refs,
			Self::Refs => Self::Check,
			Self::Check => Self::Change,
			Self::Change => Self::Overview,
		}
	}

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
	Usages,
	Change,
}

impl VisualizationMode {
	pub(in crate::ui) fn label(self) -> &'static str {
		match self {
			Self::Explorer => "explorer",
			Self::Search => "search",
			Self::Usages => "usages",
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum ActiveFilter {
	None,
	Text { raw: String, query: SymbolFilter },
	Invalid { raw: String, error: String },
	Search { raw: String, hits: Vec<SearchHit> },
	Usages(UsageFocus),
	Change,
}

impl Default for ActiveFilter {
	fn default() -> Self {
		Self::None
	}
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
	pub(in crate::ui) active_filter: ActiveFilter,
	pub(in crate::ui) filter_draft: String,
	pub(in crate::ui) search_draft: String,
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
			active_filter: ActiveFilter::None,
			filter_draft: String::new(),
			search_draft: String::new(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum CheckState {
	Pending,
	Ready(CheckSummary),
	Error(String),
}

impl Default for CheckState {
	fn default() -> Self {
		Self::Pending
	}
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
				Transition::changed("shell.set_status")
			}
			ShellAction::AppendStatus(status) => {
				self.append_status(status);
				Transition::changed("shell.append_status")
			}
			ShellAction::SetCheckState(state) => {
				self.set_check_state(state.clone());
				Transition::changed("shell.set_check_state")
			}
			ShellAction::SetRoute(route) => {
				self.update_shell(|shell| shell.route = route.clone());
				Transition::changed("shell.set_route")
			}
			ShellAction::SetView {
				view,
				policy,
				route,
			} => {
				self.update_shell(|shell| {
					shell.view = *view;
					shell.panel_policy = *policy;
					shell.route = route.clone();
				});
				Transition::changed("shell.set_view")
			}
			ShellAction::SetActiveFilter {
				active_filter,
				view_mode,
				panel_policy,
				mode,
				change_panel,
				clear_filter_draft,
				clear_search_draft,
			} => {
				self.update_shell(|shell| {
					shell.mode = *mode;
					shell.active_filter = active_filter.clone();
					shell.view_mode = *view_mode;
					shell.panel_policy = *panel_policy;
					if let Some(change_panel) = change_panel {
						shell.change_panel = *change_panel;
					}
					if *clear_filter_draft {
						shell.filter_draft.clear();
					}
					if *clear_search_draft {
						shell.search_draft.clear();
					}
				});
				Transition::changed("shell.set_active_filter")
			}
			ShellAction::ReplaceActiveFilter(active_filter) => {
				self.update_shell(|shell| shell.active_filter = active_filter.clone());
				Transition::changed("shell.replace_active_filter")
			}
			ShellAction::SetChangePanel(change_panel) => {
				self.update_shell(|shell| shell.change_panel = *change_panel);
				Transition::changed("shell.set_change_panel")
			}
		}
	}

	pub(in crate::ui) fn reduce_ui_msg(&mut self, msg: &Msg) -> Transition {
		match msg {
			Msg::Quit => Transition::unchanged("ui.quit").with_effect(Effect::Quit),
			Msg::CycleView => Transition::unchanged("ui.cycle_view")
				.with_effect(Effect::Navigate(self.shell.view.next().route())),
			Msg::ShowView(view) => {
				Transition::unchanged("ui.show_view").with_effect(Effect::Navigate(view.route()))
			}
			Msg::StartFilterEdit => {
				let draft = self
					.shell
					.active_filter
					.text_raw()
					.map(str::to_string)
					.unwrap_or_default();
				self.update_shell(|shell| {
					shell.mode = UiMode::EditingFilter;
					shell.filter_draft = draft;
				});
				self.shell.status =
					"type a structural filter, Enter applies, Esc cancels: Resolver, kind:interface, kind:method async.*"
						.to_string();
				Transition::changed("ui.start_filter_edit")
			}
			Msg::StartSearchEdit => {
				let draft = match &self.shell.active_filter {
					ActiveFilter::Search { raw, .. } => raw.clone(),
					_ => String::new(),
				};
				self.update_shell(|shell| {
					shell.mode = UiMode::EditingSearch;
					shell.search_draft = draft;
				});
				self.shell.status =
					"type a symbol search, Enter applies, Esc cancels: customer resolver format"
						.to_string();
				Transition::changed("ui.start_search_edit")
			}
			Msg::FilterInput(edit) => {
				let label = self.edit_input(*edit);
				let draft = match self.shell.mode {
					UiMode::EditingSearch => self.shell.search_draft.as_str(),
					UiMode::EditingFilter | UiMode::Normal => self.shell.filter_draft.as_str(),
				};
				self.shell.status = format!("draft {label}: {}", display_filter_text(draft));
				Transition::changed("ui.edit_input")
			}
			Msg::CancelInput => {
				let input = match self.shell.mode {
					UiMode::EditingSearch => "search",
					UiMode::EditingFilter | UiMode::Normal => "filter",
				};
				self.update_shell(|shell| shell.mode = UiMode::Normal);
				self.shell.status = format!(
					"{input} edit canceled; active filter: {}",
					self.filter_label()
				);
				Transition::changed("ui.cancel_input")
			}
			Msg::Help => {
				self.set_status(
					"keys: Enter/right open, Esc/left close, / filter, s search, d changes, u usages, y copy panel, x clear, Tab/1-5 panels, c check, q quit",
				);
				Transition::changed("ui.help")
			}
			Msg::ApplyFilter => run_command(AppCommand::ApplyFilter),
			Msg::ApplySearch => run_command(AppCommand::ApplySearch),
			Msg::ClearFilter => run_command(AppCommand::ClearFilter),
			Msg::FocusUsages => run_command(AppCommand::FocusUsages),
			Msg::ToggleChangeMode => run_command(AppCommand::ToggleChangeMode),
			Msg::CopyPanelSnapshot => run_command(AppCommand::CopyPanelSnapshot),
			Msg::RunCheck => run_command(AppCommand::RunCheck),
			Msg::MoveDown => run_command(AppCommand::Navigation(NavigationAction::MoveDown)),
			Msg::MoveUp => run_command(AppCommand::Navigation(NavigationAction::MoveUp)),
			Msg::Home => run_command(AppCommand::Navigation(NavigationAction::Home)),
			Msg::End => run_command(AppCommand::Navigation(NavigationAction::End)),
			Msg::ToggleNode => run_command(AppCommand::ToggleSelectedNode),
			Msg::OpenNode => run_command(AppCommand::OpenSelectedNode),
			Msg::CloseNode => run_command(AppCommand::CloseNodeOrClearScope),
			Msg::Noop => Transition::unchanged("ui.noop"),
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

	pub(in crate::ui) fn complete_task(&mut self, result: &TaskResult) -> bool {
		let accepted = self.accepts_task_result(result);
		self.bump();
		self.work.running.remove(&result.id);
		if !accepted {
			return false;
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
			TaskOutcome::Completed(_) => {
				self.work.pending.remove(&result.work);
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
				TaskOutcome::Completed(_) => TaskStatus::Completed,
				TaskOutcome::FileCatalogLoaded(_) => TaskStatus::Completed,
				TaskOutcome::StoreReloaded(_) => TaskStatus::Completed,
				TaskOutcome::GitOverlayRefreshed(_) => TaskStatus::Completed,
				TaskOutcome::CheckCompleted(_) => TaskStatus::Completed,
				TaskOutcome::Failed(_) => TaskStatus::Failed,
			},
		});
		true
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

	fn edit_input(&mut self, edit: FilterEdit) -> &'static str {
		let mut edited = "filter";
		self.update_shell(|shell| {
			let (draft, label) = match shell.mode {
				UiMode::EditingSearch => (&mut shell.search_draft, "search"),
				UiMode::EditingFilter | UiMode::Normal => (&mut shell.filter_draft, "filter"),
			};
			edited = label;
			match edit {
				FilterEdit::Push(c) => draft.push(c),
				FilterEdit::Backspace => {
					draft.pop();
				}
				FilterEdit::Clear => draft.clear(),
			}
		});
		edited
	}

	fn filter_label(&self) -> String {
		if self.shell.mode == UiMode::EditingFilter {
			return display_filter_text(&self.shell.filter_draft).to_string();
		}
		if self.shell.mode == UiMode::EditingSearch {
			return format!("search:{}", display_filter_text(&self.shell.search_draft));
		}
		self.shell.active_filter.label()
	}
}

impl ActiveFilter {
	pub(in crate::ui) fn label(&self) -> String {
		match self {
			Self::None => "<all>".to_string(),
			Self::Text { query, .. } => query.describe(),
			Self::Invalid { raw, .. } => display_filter_text(raw).to_string(),
			Self::Search { raw, .. } => format!("search:{raw}"),
			Self::Usages(focus) => format!("usages:{}", focus.label),
			Self::Change => "changes".to_string(),
		}
	}

	pub(in crate::ui) fn text_raw(&self) -> Option<&str> {
		match self {
			Self::Text { raw, .. } | Self::Invalid { raw, .. } => Some(raw),
			Self::None | Self::Search { .. } | Self::Usages(_) | Self::Change => None,
		}
	}
}

fn display_filter_text(filter: &str) -> &str {
	if filter.is_empty() { "<empty>" } else { filter }
}

fn run_command(command: AppCommand) -> Transition {
	Transition::unchanged("ui.command").with_effect(Effect::RunCommand(command))
}
