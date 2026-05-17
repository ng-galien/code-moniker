use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use code_moniker_core::lang::Lang;
use crossterm::event::{Event, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
	EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
#[cfg(test)]
use ratatui::text::Line;

use crate::args::UiArgs;
use crate::workspace::{
	ChangeDetail, DefLocation, IndexStore, SessionOptions, StoreWatchRoot, UsageFocus,
	WorkspaceStore,
};
use crate::{DEFAULT_SCHEME, Exit};

mod app;
mod clipboard;
mod component;
mod contracts;
mod events;
mod features;
mod kinds;
mod live;
mod navigator;
mod panel;
mod panels;
mod reactive;
mod runtime;
mod shell;
mod source;
mod store;
#[cfg(test)]
mod tests;
mod text;
mod theme;
mod view;

use app::{
	ActiveFilter, AppAction, AppCommand, AppStore, ChangePanelMode, CheckState, Effect,
	HeaderSearchResults, HeaderSearchState, PanelPolicy, ShellAction, TaskCompletion, View,
	VisualizationMode,
};
#[cfg(test)]
use component::{ComponentId, block_title};
use contracts::Route;
use events::{HeaderSearchFocus, UiMode, key_to_msg};
use features::explorer::ExplorerFeature;
#[cfg(test)]
use features::explorer::{ROUTE_OUTLINE, ROUTE_OVERVIEW, ROUTE_REFS};
use live::StoreEvent;
use navigator::{NavNodeKind, NavRow, build_change_navigator, build_navigator};
use runtime::{TaskOutcome, TaskRuntime};
use shell::{EventSource, FeatureRegistry, ShellEvent};
use store::navigation::{NavigationAction, NavigationNotice, NavigationScope, NavigationState};
#[cfg(test)]
use view::{
	active_panel_snapshot, change_panel_lines, header_line, nav_row_line, refs_panel_lines,
	render_shell, search_input_title, search_input_value, search_input_visible,
};

const DEFAULT_PANEL_SNAPSHOT_WIDTH: usize = 100;
const HEADER_SEARCH_DEBOUNCE_MS: u64 = 180;

pub fn run<W1: Write, W2: Write>(args: &UiArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match run_inner(args, stdout) {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn run_inner<W: Write>(args: &UiArgs, stdout: &mut W) -> anyhow::Result<()> {
	let scheme = args.scheme.as_deref().unwrap_or(DEFAULT_SCHEME).to_string();
	let opts = SessionOptions {
		paths: args.paths.clone(),
		project: args.project.clone(),
		cache_dir: args.cache.clone(),
	};
	let app = App::boot(opts, scheme, args.rules.clone(), args.profile.clone());
	run_terminal(stdout, app)
}

fn run_terminal<W: Write>(stdout: &mut W, mut app: App) -> anyhow::Result<()> {
	enable_raw_mode()?;
	if let Err(error) = execute!(stdout, EnterAlternateScreen) {
		let _ = disable_raw_mode();
		return Err(error.into());
	}
	let result = (|| -> anyhow::Result<()> {
		let backend = CrosstermBackend::new(&mut *stdout);
		let mut terminal = Terminal::new(backend)?;
		let result = app_loop(&mut terminal, &mut app);
		let _ = terminal.show_cursor();
		result
	})();
	let _ = disable_raw_mode();
	let _ = execute!(stdout, LeaveAlternateScreen);
	result
}

fn app_loop<W: Write>(
	terminal: &mut Terminal<CrosstermBackend<&mut W>>,
	app: &mut App,
) -> anyhow::Result<()> {
	let mut events = EventSource::start(app.store().watch_roots());
	app.set_event_sender(events.sender());
	if let Some(status) = events.status.as_deref() {
		app.set_status(status);
	}
	app.queue_startup_load();
	terminal.draw(|frame| view::draw(frame, app))?;
	loop {
		let batch = events.recv_batch()?;
		if handle_app_events(batch, app)? {
			return Ok(());
		}
		if let Some(watch_roots) = app.take_watch_roots_update() {
			if let Some(status) = events.replace_watch_roots(watch_roots) {
				app.append_status(status);
			}
		}
		terminal.draw(|frame| view::draw(frame, app))?;
	}
}

fn handle_app_events(events: Vec<ShellEvent>, app: &mut App) -> anyhow::Result<bool> {
	let mut store_event: Option<StoreEvent> = None;
	for event in events {
		match event {
			ShellEvent::Terminal(Event::Key(key)) if key.kind == KeyEventKind::Press => {
				if app.handle_key(key)? {
					return Ok(true);
				}
			}
			ShellEvent::Terminal(_) => {}
			ShellEvent::Store(event) => {
				store_event = Some(match store_event {
					Some(current) => current.coalesce(event),
					None => event,
				});
			}
			ShellEvent::TaskCompleted(result) => {
				if app.update(AppAction::TaskCompleted(result)) {
					return Ok(true);
				}
			}
			ShellEvent::HeaderSearchDebounced(generation) => {
				if app.update(AppAction::HeaderSearchDebounced(generation)) {
					return Ok(true);
				}
			}
			ShellEvent::Clipboard(result) => {
				if app.update(AppAction::Clipboard(result)) {
					return Ok(true);
				}
			}
			ShellEvent::Error(error) => return Err(anyhow::anyhow!(error)),
		}
	}
	if let Some(event) = store_event {
		if app.update(AppAction::Store(event)) {
			return Ok(true);
		}
	}
	Ok(false)
}

impl ActiveFilter {
	fn usage_focus(&self) -> Option<&UsageFocus> {
		match self {
			Self::Usages(focus) => Some(focus),
			Self::None | Self::HeaderSearch(_) | Self::Change => None,
		}
	}

	fn filters_navigator(&self) -> bool {
		matches!(self, Self::HeaderSearch(_) | Self::Usages(_) | Self::Change)
	}
}

struct App {
	app_store: AppStore,
	registry: FeatureRegistry,
	scheme: String,
	rules: PathBuf,
	profile: Option<String>,
	event_tx: Option<Sender<ShellEvent>>,
	startup_load_pending: bool,
	watch_roots_update: Option<Vec<StoreWatchRoot>>,
}

impl App {
	fn new(store: WorkspaceStore, scheme: String, rules: PathBuf, profile: Option<String>) -> Self {
		let registry = FeatureRegistry::static_registry();
		let route = registry.initial_route();
		let nav_count = registry.navigation().len();
		let command_count = registry.commands().len();
		let navigator = build_navigator(&store);
		let change_navigator = build_change_navigator(&store);
		let mut app_store = AppStore::from_workspace_store(store);
		app_store.set_navigation(NavigationState::new(navigator, change_navigator));
		let mut app = Self {
			app_store,
			registry,
			scheme,
			rules,
			profile,
			event_tx: None,
			startup_load_pending: false,
			watch_roots_update: None,
		};
		app.dispatch_shell(ShellAction::SetRoute(route));
		app.set_status(format!(
			"Enter opens nodes, Esc/left closes, s focuses search, x resets filters, d changes, u usages, y copies panel, c checks, q quits ({nav_count} nav items, {command_count} commands)"
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
		self.dispatch_and_apply(&AppAction::Shell(action));
	}

	fn route(&self) -> &Route {
		&self.app_store.shell().route
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

	fn selected(&self) -> Option<DefLocation> {
		self.selected_nav_row().and_then(|row| match row.kind {
			NavNodeKind::Def(loc) => Some(loc),
			_ => None,
		})
	}

	fn selected_change_detail(&self) -> Option<ChangeDetail> {
		self.selected_nav_row().and_then(|row| match row.kind {
			NavNodeKind::Change(id) => self.store().change_detail(id),
			NavNodeKind::Def(loc) => self.store().change_detail_for_symbol(&loc),
			_ => None,
		})
	}

	fn selected_nav_row(&self) -> Option<&NavRow> {
		self.app_store.navigation().selected_row()
	}

	fn active_expanded(&self) -> &BTreeSet<store::ids::NodeId> {
		self.app_store.navigation().active_expanded()
	}

	fn nav_rows(&self) -> &[NavRow] {
		self.app_store.navigation().rows()
	}

	fn visible_defs(&self) -> &[DefLocation] {
		self.app_store.navigation().visible_defs()
	}

	fn selected_nav_index(&self) -> usize {
		self.app_store.navigation().selection()
	}

	fn dispatch_navigation(&mut self, action: NavigationAction) -> bool {
		let (changed, effects) = {
			let transition = self.app_store.dispatch_navigation(action);
			(transition.changed, transition.take_effects())
		};
		self.apply_effects(effects);
		changed
	}

	fn refresh_results(&mut self, reset_expansion: bool) {
		let visible_defs = self.matching_defs();
		let expand_symbols = visible_defs.len() <= 200;
		self.dispatch_navigation(NavigationAction::SetScope {
			scope: self.navigation_scope(),
			visible_defs,
			reset_expansion,
			expand_symbols,
		});
	}

	fn matching_defs(&self) -> Vec<DefLocation> {
		match self.active_filter() {
			ActiveFilter::HeaderSearch(results) => results.matches.clone(),
			ActiveFilter::Usages(focus) => focus.contexts.clone(),
			ActiveFilter::Change => self.store().changed_defs(),
			ActiveFilter::None => self.store().all_navigable_defs(),
		}
	}

	fn navigation_scope(&self) -> NavigationScope {
		if matches!(self.active_filter(), ActiveFilter::Change) {
			NavigationScope::Change
		} else if self.is_filtered() {
			NavigationScope::Filtered
		} else {
			NavigationScope::Explorer
		}
	}

	fn select_def(&mut self, loc: DefLocation) {
		self.dispatch_navigation(NavigationAction::SelectDef(loc));
	}

	fn select_first_change(&mut self) {
		self.dispatch_navigation(NavigationAction::SelectFirstChange);
	}

	fn filter_label(&self) -> String {
		if matches!(self.mode(), UiMode::HeaderSearch(_)) {
			let header = self.header_search();
			return header_search_label(&header.text, header.lang, header.kind.as_deref());
		}
		self.active_filter().label()
	}

	fn is_filtered(&self) -> bool {
		self.active_filter().filters_navigator()
	}

	fn has_clearable_scope(&self) -> bool {
		!matches!(self.active_filter(), ActiveFilter::None)
	}

	fn contextual_view(&self) -> View {
		match self.view_mode() {
			VisualizationMode::Usages => View::Refs,
			VisualizationMode::Change => View::Change,
			VisualizationMode::Explorer | VisualizationMode::Search => {
				if self.selected().is_some() {
					View::Tree
				} else {
					View::Overview
				}
			}
		}
	}

	fn sync_contextual_view(&mut self) {
		if self.panel_policy() == PanelPolicy::Contextual {
			self.set_view(self.contextual_view(), PanelPolicy::Contextual);
		}
	}

	fn set_view(&mut self, view: View, policy: PanelPolicy) {
		self.dispatch_shell(ShellAction::SetView {
			view,
			policy,
			route: view.route(),
		});
	}

	fn scope_label(&self) -> String {
		match self.active_filter() {
			ActiveFilter::None => "all".to_string(),
			ActiveFilter::HeaderSearch(results) => results.label(),
			ActiveFilter::Usages(focus) => focus.label.clone(),
			ActiveFilter::Change => self.store().change_overview().scope,
		}
	}

	fn focus_usages(&mut self, loc: DefLocation) {
		let focus = self.store().usage_focus(loc);
		let label = focus.label.clone();
		let refs_len = focus.refs.len();
		let contexts_len = focus.contexts.len();
		self.dispatch_shell(ShellAction::FocusUsages(focus));
		self.refresh_results(true);
		self.sync_contextual_view();
		self.set_status(format!(
			"usages of {label}: {} reference(s), {} navigable context(s)",
			refs_len, contexts_len
		));
	}

	fn apply_header_search(&mut self, generation: Option<u64>, return_focus: bool) {
		if generation.is_some() && generation != self.header_search().pending_generation {
			return;
		}
		let header = self.header_search().clone();
		if !header.has_filter() {
			self.clear_filter_with_focus(return_focus);
			if return_focus {
				self.dispatch_shell(ShellAction::SetStatus("search cleared".to_string()));
			}
			return;
		}
		let results = self.header_search_results(&header.text, header.lang, header.kind.as_deref());
		let match_count = results.matches.len();
		let first_match = results.matches.first().copied();
		self.dispatch_shell(ShellAction::ApplyHeaderSearch {
			results: results.clone(),
			return_focus,
		});
		self.refresh_results(true);
		if let Some(loc) = first_match {
			self.select_def(loc);
		}
		self.sync_contextual_view();
		if return_focus {
			self.dispatch_shell(ShellAction::SetStatus(format!(
				"search applied: {} ({}/{})",
				results.label(),
				match_count,
				self.store().stats().defs
			)));
		} else {
			self.set_status(format!(
				"search: {} ({}/{})",
				results.label(),
				match_count,
				self.store().stats().defs
			));
		}
	}

	fn header_search_results(
		&self,
		text: &str,
		lang: Option<Lang>,
		kind: Option<&str>,
	) -> HeaderSearchResults {
		let raw = text.trim().to_string();
		let mut matches = if raw.is_empty() {
			self.store().all_navigable_defs()
		} else {
			self.store()
				.search_symbols_filtered(&raw, 500, lang, kind)
				.into_iter()
				.map(|hit| hit.loc)
				.collect()
		};
		matches.retain(|loc| {
			let symbol = self.store().symbol_summary(loc);
			self.store().is_navigable_symbol(loc)
				&& lang.is_none_or(|lang| symbol.lang == lang)
				&& kind.is_none_or(|kind| symbol.kind == kind)
		});
		HeaderSearchResults {
			text: raw,
			lang,
			kind: kind.map(str::to_string),
			matches,
		}
	}

	fn cycle_header_search_selector(&mut self, direction: i8) {
		let focus = match self.mode() {
			UiMode::HeaderSearch(focus) => focus,
			UiMode::Normal => HeaderSearchFocus::Text,
		};
		match focus {
			HeaderSearchFocus::Text => {
				self.dispatch_shell(ShellAction::SetStatus(
					"type text or press Tab to edit language".to_string(),
				));
			}
			HeaderSearchFocus::Lang => {
				let next_lang = cycle_optional(
					self.header_search().lang,
					self.available_header_langs(),
					direction,
				);
				let next_kind = self
					.header_search()
					.kind
					.as_deref()
					.filter(|kind| self.kind_is_available(next_lang, kind))
					.map(str::to_string);
				self.dispatch_shell(ShellAction::SetHeaderSearchFilters {
					lang: next_lang,
					kind: next_kind,
				});
				self.set_status(format!(
					"language filter: {}",
					next_lang.map_or("all", Lang::tag)
				));
			}
			HeaderSearchFocus::Kind => {
				let next_kind = cycle_optional(
					self.header_search().kind.clone(),
					self.available_header_kinds(self.header_search().lang),
					direction,
				);
				self.dispatch_shell(ShellAction::SetHeaderSearchFilters {
					lang: self.header_search().lang,
					kind: next_kind.clone(),
				});
				self.set_status(format!(
					"kind filter: {}",
					next_kind.as_deref().unwrap_or("all")
				));
			}
		}
	}

	fn available_header_langs(&self) -> Vec<Lang> {
		Lang::ALL
			.iter()
			.copied()
			.filter(|lang| self.store().stats().by_lang.contains_key(lang.tag()))
			.collect()
	}

	fn available_header_kinds(&self, lang: Option<Lang>) -> Vec<String> {
		let mut kinds = BTreeSet::new();
		for loc in self.store().all_navigable_defs() {
			let symbol = self.store().symbol_summary(&loc);
			if lang.is_none_or(|lang| symbol.lang == lang) {
				kinds.insert(symbol.kind);
			}
		}
		kinds.into_iter().collect()
	}

	fn kind_is_available(&self, lang: Option<Lang>, kind: &str) -> bool {
		self.available_header_kinds(lang)
			.iter()
			.any(|available| available == kind)
	}

	fn clear_filter(&mut self) {
		self.clear_filter_with_focus(true);
	}

	fn clear_filter_with_focus(&mut self, return_focus: bool) {
		self.dispatch_shell(ShellAction::ClearFilter { return_focus });
		self.refresh_results(true);
		self.sync_contextual_view();
		self.set_status("filter cleared");
	}

	fn focus_usages_of_selected(&mut self) {
		if self.view_mode() == VisualizationMode::Change {
			self.toggle_change_usages();
			return;
		}
		let Some(loc) = self.selected() else {
			self.set_status("select a declaration before focusing usages");
			return;
		};
		self.focus_usages(loc);
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
				runtime::TaskSpec::refresh_git_overlay(self.store().git_overlay_refresh_input())
			}
			StoreEvent::FullIndex => runtime::TaskSpec::reload_store(self.store().options()),
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
		let reset =
			matches!(self.active_filter(), ActiveFilter::Change) && self.nav_rows().is_empty();
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
		self.watch_roots_update = Some(self.store().watch_roots());
		let reset =
			matches!(self.active_filter(), ActiveFilter::Change) && self.nav_rows().is_empty();
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
				self.header_search_results(&results.text, results.lang, results.kind.as_deref()),
			),
			ActiveFilter::Usages(focus) => ActiveFilter::Usages(
				self.store()
					.usage_focus_for_target(focus.target.clone(), focus.label.clone()),
			),
			ActiveFilter::None => ActiveFilter::None,
			ActiveFilter::Change => ActiveFilter::Change,
		};
		self.dispatch_shell(ShellAction::ReplaceActiveFilter(active_filter));
	}

	fn toggle_selected_nav(&mut self) {
		self.dispatch_navigation(NavigationAction::ToggleSelected);
		match self.app_store.navigation().last_notice() {
			NavigationNotice::Opened(label) => self.set_status(format!("opened {label}")),
			NavigationNotice::Closed(label) => self.set_status(format!("closed {label}")),
			NavigationNotice::MovedToParent | NavigationNotice::Noop => {}
		}
	}

	fn open_selected_nav(&mut self) {
		self.dispatch_navigation(NavigationAction::OpenSelected);
		if let NavigationNotice::Opened(label) = self.app_store.navigation().last_notice() {
			self.set_status(format!("opened {label}"));
		}
	}

	fn close_selected_nav(&mut self) -> bool {
		self.dispatch_navigation(NavigationAction::CloseSelected);
		match self.app_store.navigation().last_notice() {
			NavigationNotice::Closed(label) => {
				self.set_status(format!("closed {label}"));
				true
			}
			NavigationNotice::MovedToParent => {
				self.sync_contextual_view();
				true
			}
			NavigationNotice::Opened(_) | NavigationNotice::Noop => false,
		}
	}

	fn run_check(&mut self) {
		self.set_view(View::Check, PanelPolicy::Manual);
		let task = runtime::TaskSpec::run_check(
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
		if self.queue_task(runtime::TaskSpec::load_file_catalog(self.store().options())) {
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

	fn handle_task_result(&mut self, result: runtime::TaskResult) {
		match result.outcome {
			#[cfg(test)]
			TaskOutcome::Completed(message) => {
				self.set_status(format!("{} completed: {message}", result.label));
			}
			TaskOutcome::FileCatalogLoaded(store) => {
				self.replace_store(*store);
				self.apply_file_catalog_store("file tree ready".to_string());
				if self.queue_task(runtime::TaskSpec::reload_store(self.store().options())) {
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

	fn copy_panel_snapshot(&mut self) {
		let panel = ExplorerFeature::active_panel(self);
		let snapshot = panels::panel_snapshot(&panel, view::current_panel_snapshot_width());
		let component = snapshot.component.as_str().to_string();
		let text = snapshot.to_text(self.view_mode().label(), &self.scope_label());
		let Some(tx) = self.event_tx.clone() else {
			self.set_status("clipboard copy unavailable before event loop start");
			return;
		};
		match clipboard::copy_text_async(component.clone(), text, move |result| {
			let _ = tx.send(ShellEvent::Clipboard(result));
		}) {
			Ok(()) => self.set_status(format!("copying {component} snapshot to clipboard")),
			Err(error) => self.set_status(format!("clipboard copy failed: {error:#}")),
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
			Effect::Navigate(route) => self.navigate(route),
			Effect::Quit => return true,
			#[cfg(test)]
			Effect::Notify(message) => self.set_status(message),
			#[cfg(test)]
			Effect::Spawn(task) => {
				self.queue_task(task);
			}
			Effect::DebounceHeaderSearch(generation) => {
				self.queue_header_search_debounce(generation);
			}
			Effect::RunCommand(command) => return self.run_command(command),
		}
		false
	}

	fn run_command(&mut self, command: AppCommand) -> bool {
		match command {
			AppCommand::ApplyHeaderSearch {
				generation,
				return_focus,
			} => self.apply_header_search(generation, return_focus),
			AppCommand::CycleHeaderSearchSelector { direction } => {
				self.cycle_header_search_selector(direction)
			}
			AppCommand::FocusUsages => self.focus_usages_of_selected(),
			AppCommand::ToggleChangeMode => self.toggle_change_mode(),
			AppCommand::CopyPanelSnapshot => self.copy_panel_snapshot(),
			AppCommand::RunCheck => self.run_check(),
			AppCommand::Navigation(action) => self.apply_navigation(*action),
			AppCommand::ToggleSelectedNode => self.toggle_selected_nav(),
			AppCommand::OpenSelectedNode => self.open_selected_nav(),
			AppCommand::CloseNodeOrClearScope => {
				if !self.close_selected_nav() && self.has_clearable_scope() {
					self.clear_filter();
				}
			}
		}
		false
	}

	fn apply_navigation(&mut self, action: NavigationAction) {
		let changed = self.dispatch_navigation(action);
		if changed {
			self.sync_contextual_view();
		}
	}

	fn queue_task(&mut self, task: runtime::TaskSpec) -> bool {
		let Some(tx) = self.event_tx.clone() else {
			let label = task.label().to_string();
			self.set_status(format!("task runtime unavailable for {label}"));
			return false;
		};
		let task = self.app_store.register_task(task);
		let label = task.label().to_string();
		let id = task.id();
		TaskRuntime::spawn(task, move |result| {
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

	fn navigate(&mut self, route: Route) {
		if !self.registry.can_open(&route) {
			self.set_status(format!("unknown route: {}/{}", route.feature, route.path));
			return;
		}
		if let Some(view) = View::from_route_path(&route.path) {
			self.set_view(view, PanelPolicy::Manual);
			return;
		}
		self.dispatch_shell(ShellAction::SetRoute(route));
	}
}

fn display_filter(filter: &str) -> &str {
	if filter.is_empty() { "<all>" } else { filter }
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

fn cycle_optional<T: Clone + Eq>(current: Option<T>, options: Vec<T>, direction: i8) -> Option<T> {
	let len = options.len() + 1;
	if len == 1 {
		return None;
	}
	let current_idx = current
		.as_ref()
		.and_then(|current| options.iter().position(|option| option == current))
		.map(|idx| idx + 1)
		.unwrap_or(0);
	let next = if direction >= 0 {
		(current_idx + 1) % len
	} else {
		(current_idx + len - 1) % len
	};
	if next == 0 {
		None
	} else {
		Some(options[next - 1].clone())
	}
}
