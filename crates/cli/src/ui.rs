use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use crossterm::event::KeyEvent;

use crate::Exit;
use crate::args::UiArgs;
use crate::workspace::{
	DefLocation, IndexStore, SessionOptions, StoreWatchRoot, UsageFocus, WorkspaceStore,
};

mod app;
mod async_task;
mod clipboard;
mod events;
mod explorer;
mod live;
mod panel;
mod render;
mod shell;
mod store;

use app::{
	ActiveFilter, AppAction, AppStore, ChangePanelMode, CheckState, Effect, FocusRegion,
	HeaderSearchState, PanelPolicy, ShellAction, TaskCompletion, View, VisualizationMode,
};
use async_task::{TaskOutcome, TaskResult, TaskRunner, TaskSpec};
use events::{UiMode, key_to_msg};
use live::StoreEvent;
use shell::ShellEvent;
use store::navigation::{NavigationAction, NavigationState};
use store::navigation_tree::{build_change_navigator, build_navigator};

const DEFAULT_PANEL_SNAPSHOT_WIDTH: usize = 100;
const HEADER_SEARCH_DEBOUNCE_MS: u64 = 180;

pub fn run<W1: Write, W2: Write>(args: &UiArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	shell::terminal::run(args, stdout, stderr)
}

impl ActiveFilter {
	fn filters_navigator(&self) -> bool {
		matches!(self, Self::HeaderSearch(_) | Self::Change)
	}
}

struct App {
	app_store: AppStore,
	scheme: String,
	rules: PathBuf,
	profile: Option<String>,
	event_tx: Option<Sender<ShellEvent>>,
	startup_load_pending: bool,
	watch_roots_update: Option<Vec<StoreWatchRoot>>,
}

impl App {
	fn new(store: WorkspaceStore, scheme: String, rules: PathBuf, profile: Option<String>) -> Self {
		let navigator = build_navigator(&store);
		let change_navigator = build_change_navigator(&store);
		let mut app_store = AppStore::from_workspace_store(store);
		app_store.set_navigation(NavigationState::new(navigator, change_navigator));
		let mut app = Self {
			app_store,
			scheme,
			rules,
			profile,
			event_tx: None,
			startup_load_pending: false,
			watch_roots_update: None,
		};
		app.refresh_header_search_options();
		app.set_status(format!(
			"Enter opens nodes, Esc/left closes, PgUp/PgDn scroll panel, s focuses search, x resets filters, d changes, u usages, y copies panel, c checks, q quits"
		));
		app.refresh_results(false);
		app
	}

	fn boot(opts: SessionOptions, scheme: String, rules: PathBuf, profile: Option<String>) -> Self {
		let mut app = Self::new(WorkspaceStore::empty(opts), scheme, rules, profile);
		app.startup_load_pending = true;
		app.set_status("loading index...");
		app
	}

	fn status(&self) -> &str {
		self.app_store.status()
	}

	fn set_status(&mut self, status: impl Into<String>) {
		self.dispatch_shell(ShellAction::SetStatus(status.into()));
	}

	fn append_status(&mut self, status: impl AsRef<str>) {
		self.dispatch_shell(ShellAction::AppendStatus(status.as_ref().to_string()));
	}

	fn check_state(&self) -> &CheckState {
		self.app_store.check_state()
	}

	fn set_check_state(&mut self, state: CheckState) {
		self.dispatch_shell(ShellAction::SetCheckState(state));
	}

	fn dispatch_shell(&mut self, action: ShellAction) {
		let refresh_search_options = matches!(
			action,
			ShellAction::SetHeaderSearchFilters { .. } | ShellAction::ClearFilter { .. }
		);
		self.dispatch_and_apply(&AppAction::Shell(action));
		if refresh_search_options {
			self.refresh_header_search_options();
		}
	}

	fn view(&self) -> View {
		self.app_store.shell().view
	}

	fn view_mode(&self) -> VisualizationMode {
		self.app_store.shell().view_mode
	}

	fn panel_policy(&self) -> PanelPolicy {
		self.app_store.shell().panel_policy
	}

	fn change_panel(&self) -> ChangePanelMode {
		self.app_store.shell().change_panel
	}

	fn mode(&self) -> UiMode {
		self.app_store.shell().mode
	}

	fn focus_region(&self) -> FocusRegion {
		self.app_store.shell().focus_region
	}

	fn usage_lens(&self) -> Option<&UsageFocus> {
		self.app_store.shell().usage_lens.as_ref()
	}

	fn active_filter(&self) -> &ActiveFilter {
		&self.app_store.shell().active_filter
	}

	fn header_search(&self) -> &HeaderSearchState {
		&self.app_store.shell().header_search
	}

	fn store(&self) -> &WorkspaceStore {
		self.app_store.workspace()
	}

	fn store_mut(&mut self) -> &mut WorkspaceStore {
		self.app_store.workspace_mut()
	}

	fn replace_store(&mut self, store: WorkspaceStore) {
		self.app_store.replace_workspace(store);
	}

	fn focus_usages(&mut self, loc: DefLocation) {
		let focus = self.store().usage_focus(loc);
		let label = focus.label.clone();
		let refs_len = focus.refs.len();
		let contexts_len = focus.contexts.len();
		let visible_defs = focus.contexts.clone();
		self.dispatch_shell(ShellAction::SetUsageLens(Some(focus)));
		self.dispatch_navigation(NavigationAction::SetUsageLens {
			visible_defs,
			reset_expansion: true,
			expand_symbols: contexts_len <= 200,
		});
		self.sync_contextual_view();
		self.set_status(format!(
			"usage lens for {label}: {} reference(s), {} navigable context(s)",
			refs_len, contexts_len
		));
	}

	fn focus_usages_of_selected(&mut self) {
		if self.view_mode() == VisualizationMode::Change {
			self.toggle_change_usages();
			return;
		}
		if self.usage_lens().is_some() {
			self.close_usage_lens();
			return;
		}
		let Some(loc) = self.primary_selected() else {
			self.set_status("select a declaration before focusing usages");
			return;
		};
		self.focus_usages(loc);
	}

	fn close_usage_lens(&mut self) {
		let label = self
			.usage_lens()
			.map(|focus| focus.label.clone())
			.unwrap_or_else(|| "usage lens".to_string());
		self.dispatch_shell(ShellAction::SetUsageLens(None));
		self.dispatch_navigation(NavigationAction::ClearUsageLens);
		self.sync_contextual_view();
		self.set_status(format!("closed usage lens for {label}"));
	}

	fn toggle_change_mode(&mut self) {
		if self.view_mode() == VisualizationMode::Change {
			self.clear_filter();
			return;
		}
		self.dispatch_shell(ShellAction::EnterChangeMode);
		self.refresh_results(true);
		self.select_first_change();
		self.sync_contextual_view();
		let changes = self.store().change_overview();
		self.set_status(format!(
			"changes: {} declaration(s) across {} file(s)",
			changes.change_count, changes.file_count
		));
	}

	fn toggle_change_usages(&mut self) {
		let Some(change) = self.selected_change_detail() else {
			self.set_status("select a changed declaration before toggling blast radius");
			return;
		};
		let name = change.summary.name;
		let next_panel = match self.change_panel() {
			ChangePanelMode::Diff => ChangePanelMode::Usages,
			ChangePanelMode::Usages => ChangePanelMode::Diff,
		};
		self.dispatch_shell(ShellAction::SetChangePanel(next_panel));
		self.set_view(View::Change, PanelPolicy::Contextual);
		self.set_status(match next_panel {
			ChangePanelMode::Diff => format!("change diff details for {name}"),
			ChangePanelMode::Usages => format!("change blast radius for {name}"),
		});
	}

	fn handle_store_event(&mut self, event: StoreEvent) {
		if self.queue_store_task(event) {
			return;
		}
		self.handle_store_event_sync(event);
	}

	fn queue_store_task(&mut self, event: StoreEvent) -> bool {
		let task = match event {
			StoreEvent::GitOverlay => {
				TaskSpec::refresh_git_overlay(self.store().git_overlay_refresh_input())
			}
			StoreEvent::FullIndex => TaskSpec::reload_store(self.store().options()),
		};
		self.queue_task(task)
	}

	fn handle_store_event_sync(&mut self, event: StoreEvent) {
		match event {
			StoreEvent::GitOverlay => {
				self.store_mut().refresh_git_overlay();
				self.apply_refreshed_change_store("git overlay refreshed".to_string());
			}
			StoreEvent::FullIndex => match self.store_mut().reload() {
				Ok(()) => {
					self.apply_reloaded_store("store reloaded after filesystem change".to_string());
				}
				Err(error) => {
					self.set_status(format!("store reload failed: {error:#}"));
				}
			},
		}
	}

	fn apply_file_catalog_store(&mut self, status: String) {
		self.watch_roots_update = Some(self.store().watch_roots());
		self.refresh_header_search_options();
		self.dispatch_navigation(NavigationAction::ReplaceModels {
			explorer: build_navigator(self.store()),
			change: build_change_navigator(self.store()),
		});
		self.refresh_results(true);
		self.sync_contextual_view();
		self.set_status(status);
	}

	fn apply_reloaded_store(&mut self, status: String) {
		self.watch_roots_update = Some(self.store().watch_roots());
		self.refresh_header_search_options();
		let reset = matches!(self.active_filter(), ActiveFilter::Change)
			&& self.app_store.navigation().primary_view().rows.is_empty();
		self.refresh_active_filter_after_store_reload();
		self.dispatch_navigation(NavigationAction::ReplaceModels {
			explorer: build_navigator(self.store()),
			change: build_change_navigator(self.store()),
		});
		self.refresh_results(reset);
		if reset {
			self.select_first_change();
		}
		self.sync_contextual_view();
		self.set_status(status);
	}

	fn apply_refreshed_change_store(&mut self, status: String) {
		let reset = matches!(self.active_filter(), ActiveFilter::Change)
			&& self.app_store.navigation().primary_view().rows.is_empty();
		self.dispatch_navigation(NavigationAction::ReplaceModels {
			explorer: build_navigator(self.store()),
			change: build_change_navigator(self.store()),
		});
		self.refresh_results(reset);
		if reset {
			self.select_first_change();
		}
		self.sync_contextual_view();
		self.set_status(status);
	}

	fn refresh_active_filter_after_store_reload(&mut self) {
		let active_filter = match self.active_filter() {
			ActiveFilter::HeaderSearch(results) => ActiveFilter::HeaderSearch(
				self.header_search_results(&results.text, &results.langs, &results.kind_filters),
			),
			ActiveFilter::None => ActiveFilter::None,
			ActiveFilter::Change => ActiveFilter::Change,
		};
		self.dispatch_shell(ShellAction::ReplaceActiveFilter(active_filter));
		self.refresh_usage_lens_after_store_reload();
	}

	fn refresh_usage_lens_after_store_reload(&mut self) {
		let Some(focus) = self.usage_lens().cloned() else {
			return;
		};
		let focus = self
			.store()
			.usage_focus_for_target(focus.target, focus.label);
		let visible_defs = focus.contexts.clone();
		let expand_symbols = visible_defs.len() <= 200;
		self.dispatch_shell(ShellAction::SetUsageLens(Some(focus)));
		self.dispatch_navigation(NavigationAction::SetUsageLens {
			visible_defs,
			reset_expansion: false,
			expand_symbols,
		});
	}

	fn run_check(&mut self) {
		self.set_view(View::Check, PanelPolicy::Manual);
		let task = TaskSpec::run_check(
			self.store().clone(),
			self.rules.clone(),
			self.profile.clone(),
			self.scheme.clone(),
		);
		if self.queue_task(task) {
			self.set_status("check queued in background");
			return;
		}
		match self
			.store()
			.check_summary(&self.rules, self.profile.as_deref(), &self.scheme)
		{
			Ok(summary) => {
				self.set_status(format!(
					"check complete: {} violation(s) across {} file(s)",
					summary.total_violations, summary.files_with_violations
				));
				self.set_check_state(CheckState::Ready(summary));
			}
			Err(e) => {
				self.set_status("check failed");
				self.set_check_state(CheckState::Error(e.to_string()));
			}
		}
	}

	fn set_event_sender(&mut self, tx: Sender<ShellEvent>) {
		self.event_tx = Some(tx);
	}

	fn queue_startup_load(&mut self) {
		if !self.startup_load_pending {
			return;
		}
		self.startup_load_pending = false;
		if self.queue_task(TaskSpec::load_file_catalog(self.store().options())) {
			self.set_status("loading file tree in background");
		} else {
			self.handle_store_event_sync(StoreEvent::FullIndex);
		}
	}

	fn take_watch_roots_update(&mut self) -> Option<Vec<StoreWatchRoot>> {
		self.watch_roots_update.take()
	}

	fn handle_clipboard_result(&mut self, result: clipboard::ClipboardResult) {
		match result.result {
			Ok(()) => {
				self.set_status(format!("copied {} snapshot to clipboard", result.component));
			}
			Err(error) => {
				self.set_status(format!(
					"clipboard copy failed for {}: {error}",
					result.component
				));
			}
		}
	}

	fn handle_task_result(&mut self, result: TaskResult) {
		match result.outcome {
			TaskOutcome::FileCatalogLoaded(store) => {
				self.replace_store(*store);
				self.apply_file_catalog_store("file tree ready".to_string());
				if self.queue_task(TaskSpec::reload_store(self.store().options())) {
					self.set_status("file tree ready; loading symbols in background");
				}
			}
			TaskOutcome::StoreReloaded(store) => {
				self.replace_store(*store);
				self.apply_reloaded_store(format!("{} completed", result.label));
			}
			TaskOutcome::GitOverlayRefreshed(store) => {
				if self.store_mut().apply_git_overlay_refresh(*store) {
					self.apply_refreshed_change_store(format!("{} completed", result.label));
				} else {
					self.set_status(format!("ignored stale {} result", result.label));
				}
			}
			TaskOutcome::CheckCompleted(summary) => {
				self.set_status(format!(
					"check complete: {} violation(s) across {} file(s)",
					summary.total_violations, summary.files_with_violations
				));
			}
			TaskOutcome::Failed(error) => {
				self.set_status(format!("{} failed: {error}", result.label));
			}
		}
	}

	fn handle_key(&mut self, key: KeyEvent) -> anyhow::Result<bool> {
		Ok(self.update(AppAction::Ui(key_to_msg(self.mode(), key))))
	}

	fn update(&mut self, action: AppAction) -> bool {
		let action = match action {
			AppAction::TaskCompleted(result) => {
				match self.app_store.complete_task(&result) {
					TaskCompletion::Accepted => self.handle_task_result(result),
					TaskCompletion::Ignored => {
						self.set_status(format!("ignored stale task result: {}", result.label));
					}
				}
				return false;
			}
			action => action,
		};
		if self.dispatch_and_apply(&action) {
			return true;
		}
		match action {
			AppAction::Ui(_) => false,
			AppAction::HeaderSearchDebounced(_) => false,
			AppAction::Shell(_) => false,
			AppAction::Store(event) => {
				self.handle_store_event(event);
				false
			}
			AppAction::TaskStarted { .. } => false,
			AppAction::TaskCompleted(_) => unreachable!("task completion handled before dispatch"),
			AppAction::Clipboard(result) => {
				self.handle_clipboard_result(result);
				false
			}
		}
	}

	fn dispatch_and_apply(&mut self, action: &AppAction) -> bool {
		let effects = {
			let transition = self.app_store.dispatch(action);
			transition.take_effects()
		};
		self.apply_effects(effects)
	}

	fn apply_effects(&mut self, effects: Vec<Effect>) -> bool {
		for effect in effects {
			if self.apply_effect(effect) {
				return true;
			}
		}
		false
	}

	fn apply_effect(&mut self, effect: Effect) -> bool {
		match effect {
			Effect::ShowView(view) => self.set_view(view, PanelPolicy::Manual),
			Effect::Quit => return true,
			Effect::DebounceHeaderSearch(generation) => {
				self.queue_header_search_debounce(generation);
			}
			Effect::ApplyHeaderSearch {
				generation,
				return_focus,
			} => self.apply_header_search(generation, return_focus),
			Effect::CycleHeaderSearchSelector { direction } => {
				self.cycle_header_search_selector(direction)
			}
			Effect::ToggleHeaderSearchSelection => self.toggle_header_search_selection(),
			Effect::FocusUsages => self.focus_usages_of_selected(),
			Effect::ToggleChangeMode => self.toggle_change_mode(),
			Effect::CopyPanelSnapshot => self.copy_panel_snapshot(),
			Effect::RunCheck => self.run_check(),
			Effect::Navigation(action) => self.apply_navigation(*action),
			Effect::ToggleFocusRegion => self.toggle_focus_region(),
			Effect::PanelMove { direction } => self.move_panel_selection(direction),
			Effect::PanelHome => self.move_panel_to_edge(true),
			Effect::PanelEnd => self.move_panel_to_edge(false),
			Effect::ToggleSelectedNode => self.toggle_selected_nav(),
			Effect::OpenSelectedNode => self.open_selected_nav(),
			Effect::CloseNodeOrClearScope => {
				if !self.close_selected_nav() && self.has_clearable_scope() {
					self.clear_filter();
				}
			}
		}
		false
	}

	fn queue_task(&mut self, task: TaskSpec) -> bool {
		let Some(tx) = self.event_tx.clone() else {
			let label = task.label().to_string();
			self.set_status(format!("task runtime unavailable for {label}"));
			return false;
		};
		let task = self.app_store.register_task(task);
		let label = task.label().to_string();
		let id = task.id();
		TaskRunner::spawn(task, move |result| {
			let _ = tx.send(ShellEvent::TaskCompleted(result));
		});
		self.set_status(format!("task queued: {label} ({id})"));
		true
	}

	fn queue_header_search_debounce(&mut self, generation: u64) {
		let Some(tx) = self.event_tx.clone() else {
			return;
		};
		thread::spawn(move || {
			thread::sleep(Duration::from_millis(HEADER_SEARCH_DEBOUNCE_MS));
			let _ = tx.send(ShellEvent::HeaderSearchDebounced(generation));
		});
	}
}
