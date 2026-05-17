use std::collections::{BTreeMap, BTreeSet};

use code_moniker_core::lang::Lang;

use crate::ui::app::action::ShellAction;
use crate::ui::app::command::AppCommand;
use crate::ui::contracts::Route;
use crate::ui::events::{FilterEdit, HeaderSearchFocus, Msg, UiMode};
use crate::ui::features::explorer::{
	ExplorerFeature, ROUTE_CHANGE, ROUTE_CHECK, ROUTE_OUTLINE, ROUTE_OVERVIEW, ROUTE_REFS,
};
use crate::ui::live::StoreEvent;
use crate::ui::reactive::Transition;
use crate::ui::runtime::{TaskId, TaskOutcome, TaskResult, WorkKind};
use crate::ui::store::navigation::{NavigationAction, NavigationState};
use crate::workspace::{CheckSummary, DefLocation, UsageFocus};

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) enum ActiveFilter {
	#[default]
	None,
	HeaderSearch(HeaderSearchResults),
	Usages(UsageFocus),
	Change,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct HeaderSearchState {
	pub(in crate::ui) focus: HeaderSearchFocus,
	pub(in crate::ui) text: String,
	pub(in crate::ui) lang: Option<Lang>,
	pub(in crate::ui) kind: Option<String>,
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) pending_generation: Option<u64>,
}

impl HeaderSearchState {
	pub(in crate::ui) fn has_filter(&self) -> bool {
		!self.text.trim().is_empty() || self.lang.is_some() || self.kind.is_some()
	}

	fn reset(&mut self) {
		self.text.clear();
		self.lang = None;
		self.kind = None;
	}

	fn bump_pending(&mut self) -> u64 {
		self.generation += 1;
		self.pending_generation = Some(self.generation);
		self.generation
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct HeaderSearchResults {
	pub(in crate::ui) text: String,
	pub(in crate::ui) lang: Option<Lang>,
	pub(in crate::ui) kind: Option<String>,
	pub(in crate::ui) matches: Vec<DefLocation>,
}

impl HeaderSearchResults {
	pub(in crate::ui) fn label(&self) -> String {
		header_search_label(&self.text, self.lang, self.kind.as_deref())
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
	pub(in crate::ui) header_search: HeaderSearchState,
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
			header_search: HeaderSearchState::default(),
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
			ShellAction::ApplyHeaderSearch {
				results,
				return_focus,
			} => self.apply_header_search_action(results, *return_focus),
			ShellAction::SetHeaderSearchFilters { lang, kind } => {
				let mut generation = self.shell.header_search.generation;
				self.update_shell(|shell| {
					shell.header_search.lang = *lang;
					shell.header_search.kind = kind.clone();
					generation = shell.header_search.bump_pending();
				});
				Transition::changed("shell.set_header_search_filters")
					.with_effect(Effect::DebounceHeaderSearch(generation))
			}
			ShellAction::ClearFilter { return_focus } => {
				self.update_shell(|shell| {
					if *return_focus {
						shell.mode = UiMode::Normal;
					}
					shell.active_filter = ActiveFilter::None;
					shell.view_mode = VisualizationMode::Explorer;
					shell.panel_policy = PanelPolicy::Contextual;
					shell.change_panel = ChangePanelMode::Diff;
					shell.header_search.reset();
					shell.header_search.pending_generation = None;
				});
				Transition::changed("shell.clear_filter")
			}
			ShellAction::FocusUsages(focus) => {
				self.update_shell(|shell| {
					shell.mode = UiMode::Normal;
					shell.active_filter = ActiveFilter::Usages(focus.clone());
					shell.view_mode = VisualizationMode::Usages;
					shell.panel_policy = PanelPolicy::Contextual;
					shell.header_search.reset();
					shell.header_search.pending_generation = None;
				});
				Transition::changed("shell.focus_usages")
			}
			ShellAction::EnterChangeMode => {
				self.update_shell(|shell| {
					shell.mode = UiMode::Normal;
					shell.active_filter = ActiveFilter::Change;
					shell.view_mode = VisualizationMode::Change;
					shell.panel_policy = PanelPolicy::Contextual;
					shell.change_panel = ChangePanelMode::Diff;
					shell.header_search.reset();
					shell.header_search.pending_generation = None;
				});
				Transition::changed("shell.enter_change_mode")
			}
			ShellAction::ReplaceActiveFilter(active_filter) => {
				self.update_shell(|shell| shell.active_filter = active_filter.clone());
				Transition::changed("shell.replace_active_filter")
			}
			ShellAction::SetChangePanel(change_panel) => {
				self.update_shell(|shell| shell.change_panel = *change_panel);
				Transition::changed("shell.set_change_panel")
			}
			#[cfg(test)]
			ShellAction::EmitNotify(message) => Transition::unchanged("shell.emit_notify")
				.with_effect(Effect::Notify(message.clone())),
		}
	}

	pub(in crate::ui) fn reduce_ui_msg(&mut self, msg: &Msg) -> Transition {
		match msg {
			Msg::Quit => Transition::unchanged("ui.quit").with_effect(Effect::Quit),
			Msg::ShowView(view) => {
				Transition::unchanged("ui.show_view").with_effect(Effect::Navigate(view.route()))
			}
			Msg::ToggleHeaderSearch => {
				let next = match self.shell.mode {
					UiMode::Normal => UiMode::HeaderSearch(self.shell.header_search.focus),
					UiMode::HeaderSearch(_) => UiMode::Normal,
				};
				self.update_shell(|shell| shell.mode = next);
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
				Transition::changed("ui.toggle_header_search")
			}
			Msg::HeaderSearchNextField => {
				let focus = match self.shell.mode {
					UiMode::HeaderSearch(focus) => focus.next(),
					UiMode::Normal => HeaderSearchFocus::Text,
				};
				self.update_shell(|shell| {
					shell.header_search.focus = focus;
					shell.mode = UiMode::HeaderSearch(focus);
				});
				self.shell.status = match focus {
					HeaderSearchFocus::Text => "search text focused".to_string(),
					HeaderSearchFocus::Lang => "language selector focused".to_string(),
					HeaderSearchFocus::Kind => "kind selector focused".to_string(),
				};
				Transition::changed("ui.header_search_next_field")
			}
			Msg::HeaderSearchInput(edit) => {
				let generation = self.edit_header_search_input(*edit);
				let text = display_filter_text(&self.shell.header_search.text);
				self.shell.status = format!("search draft: {text}");
				Transition::changed("ui.header_search_input")
					.with_effect(Effect::DebounceHeaderSearch(generation))
			}
			Msg::HeaderSearchSelectNext => {
				run_command(AppCommand::CycleHeaderSearchSelector { direction: 1 })
			}
			Msg::HeaderSearchSelectPrevious => {
				run_command(AppCommand::CycleHeaderSearchSelector { direction: -1 })
			}
			Msg::HeaderSearchReset => {
				let return_focus = matches!(self.shell.mode, UiMode::Normal);
				self.reset_header_search();
				self.shell.status = "search filters reset".to_string();
				Transition::changed("ui.header_search_reset").with_effect(Effect::RunCommand(
					AppCommand::ApplyHeaderSearch {
						generation: None,
						return_focus,
					},
				))
			}
			Msg::HeaderSearchApply => run_command(AppCommand::ApplyHeaderSearch {
				generation: None,
				return_focus: true,
			}),
			Msg::Help => {
				self.set_status(
					"keys: s search focus, Tab next search field, x reset filters, Enter/right open, Esc/left close, d changes, u usages, y copy panel, 1-5 panels, c check, q quit",
				);
				Transition::changed("ui.help")
			}
			Msg::FocusUsages => run_command(AppCommand::FocusUsages),
			Msg::ToggleChangeMode => run_command(AppCommand::ToggleChangeMode),
			Msg::CopyPanelSnapshot => run_command(AppCommand::CopyPanelSnapshot),
			Msg::RunCheck => run_command(AppCommand::RunCheck),
			Msg::MoveDown => {
				run_command(AppCommand::Navigation(Box::new(NavigationAction::MoveDown)))
			}
			Msg::MoveUp => run_command(AppCommand::Navigation(Box::new(NavigationAction::MoveUp))),
			Msg::Home => run_command(AppCommand::Navigation(Box::new(NavigationAction::Home))),
			Msg::End => run_command(AppCommand::Navigation(Box::new(NavigationAction::End))),
			Msg::ToggleNode => run_command(AppCommand::ToggleSelectedNode),
			Msg::OpenNode => run_command(AppCommand::OpenSelectedNode),
			Msg::CloseNode => run_command(AppCommand::CloseNodeOrClearScope),
			Msg::Noop => Transition::unchanged("ui.noop"),
		}
	}

	pub(in crate::ui) fn reduce_header_search_debounced(&mut self, generation: u64) -> Transition {
		if self.shell.header_search.pending_generation == Some(generation) {
			run_command(AppCommand::ApplyHeaderSearch {
				generation: Some(generation),
				return_focus: false,
			})
		} else {
			Transition::unchanged("ui.header_search_debounce_stale")
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
			#[cfg(test)]
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
				#[cfg(test)]
				TaskOutcome::Completed(_) => TaskStatus::Completed,
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
			}
			shell.active_filter = ActiveFilter::HeaderSearch(results.clone());
			shell.view_mode = VisualizationMode::Search;
			shell.panel_policy = PanelPolicy::Contextual;
			shell.header_search.text = results.text.clone();
			shell.header_search.lang = results.lang;
			shell.header_search.kind = results.kind.clone();
			shell.header_search.pending_generation = None;
		});
		Transition::changed("shell.apply_header_search")
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

impl ActiveFilter {
	pub(in crate::ui) fn label(&self) -> String {
		match self {
			Self::None => "<all>".to_string(),
			Self::HeaderSearch(results) => results.label(),
			Self::Usages(focus) => format!("usages:{}", focus.label),
			Self::Change => "changes".to_string(),
		}
	}
}

fn display_filter_text(filter: &str) -> &str {
	if filter.is_empty() { "<empty>" } else { filter }
}

fn header_search_label(text: &str, lang: Option<Lang>, kind: Option<&str>) -> String {
	let mut parts = Vec::new();
	if !text.trim().is_empty() {
		parts.push(format!("search:{}", text.trim()));
	}
	if let Some(lang) = lang {
		parts.push(format!("lang:{}", lang.tag()));
	}
	if let Some(kind) = kind {
		parts.push(format!("kind:{kind}"));
	}
	if parts.is_empty() {
		"<all>".to_string()
	} else {
		parts.join(" ")
	}
}

fn run_command(command: AppCommand) -> Transition {
	Transition::unchanged("ui.command").with_effect(Effect::RunCommand(command))
}
