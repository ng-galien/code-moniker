use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use crossterm::event::{Event, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
	EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::args::UiArgs;
use crate::inspect::{DefLocation, RefLocation, SessionOptions};
use crate::lines::line_range;
use crate::{DEFAULT_SCHEME, Exit};

mod app;
mod change;
mod clipboard;
mod component;
mod contracts;
mod events;
mod features;
mod filter;
mod kinds;
mod live;
mod navigator;
mod panel;
mod reactive;
mod runtime;
mod shell;
mod source;
mod store;
#[cfg(test)]
mod tests;
mod theme;

use app::{
	ActiveFilter, AppAction, AppStore, ChangePanelMode, CheckState, PanelPolicy, ShellSlice, View,
	VisualizationMode,
};
use change::{ChangeEntry, ChangeStatus};
use component::{ComponentId, block_title, marker};
use contracts::{Effect, RenderContext, Route, Screen, ScreenContext};
use events::{FilterEdit, Msg, UiMode, key_to_msg};
use features::explorer::{
	ExplorerFeature, ROUTE_CHANGE, ROUTE_CHECK, ROUTE_OUTLINE, ROUTE_OVERVIEW, ROUTE_REFS,
};
use filter::{NavFilter, parse_filter};
use kinds::{definition_kind_group, reference_kind_group, sort_reference_kinds};
use live::StoreEvent;
use navigator::{NavNodeKind, NavRow, build_change_navigator, build_navigator};
use panel::{Column, FitMode, fit_text, visible_len};
use runtime::{TaskOutcome, TaskRuntime};
use shell::{EventSource, FeatureRegistry, ShellEvent};
use source::source_snippet_lines;
use store::navigation::{NavigationAction, NavigationNotice, NavigationScope, NavigationState};
use store::{
	IndexStore, MemoryIndexStore, StoreWatchRoot, UsageFocus, compact_moniker, def_kind, last_name,
	ref_kind,
};
use theme::THEME;

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
	terminal.draw(|frame| draw(frame, app))?;
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
		terminal.draw(|frame| draw(frame, app))?;
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
				app.update(AppAction::TaskCompleted(result));
			}
			ShellEvent::Clipboard(result) => {
				app.update(AppAction::Clipboard(result));
			}
			ShellEvent::Error(error) => return Err(anyhow::anyhow!(error)),
		}
	}
	if let Some(event) = store_event {
		app.update(AppAction::Store(event));
	}
	Ok(false)
}

impl View {
	fn next(self) -> Self {
		match self {
			Self::Overview => Self::Tree,
			Self::Tree => Self::Refs,
			Self::Refs => Self::Check,
			Self::Check => Self::Change,
			Self::Change => Self::Overview,
		}
	}

	fn route_path(self) -> &'static str {
		match self {
			Self::Overview => ROUTE_OVERVIEW,
			Self::Tree => ROUTE_OUTLINE,
			Self::Refs => ROUTE_REFS,
			Self::Check => ROUTE_CHECK,
			Self::Change => ROUTE_CHANGE,
		}
	}

	fn from_route_path(path: &str) -> Option<Self> {
		match path {
			ROUTE_OVERVIEW => Some(Self::Overview),
			ROUTE_OUTLINE => Some(Self::Tree),
			ROUTE_REFS => Some(Self::Refs),
			ROUTE_CHECK => Some(Self::Check),
			ROUTE_CHANGE => Some(Self::Change),
			_ => None,
		}
	}

	fn route(self) -> Route {
		ExplorerFeature::route(self.route_path())
	}
}

impl VisualizationMode {
	fn label(self) -> &'static str {
		match self {
			Self::Explorer => "explorer",
			Self::Search => "search",
			Self::Usages => "usages",
			Self::Change => "change",
		}
	}
}

impl ActiveFilter {
	fn label(&self) -> String {
		match self {
			Self::None => "<all>".to_string(),
			Self::Text { query, .. } => query.describe(),
			Self::Invalid { raw, .. } => display_filter(raw).to_string(),
			Self::Search { raw, .. } => format!("search:{raw}"),
			Self::Usages(focus) => format!("usages:{}", focus.label),
			Self::Change => "changes".to_string(),
		}
	}

	fn text_raw(&self) -> Option<&str> {
		match self {
			Self::Text { raw, .. } | Self::Invalid { raw, .. } => Some(raw),
			Self::None | Self::Search { .. } | Self::Usages(_) | Self::Change => None,
		}
	}

	fn query(&self) -> Option<&NavFilter> {
		match self {
			Self::Text { query, .. } => Some(query),
			Self::None
			| Self::Invalid { .. }
			| Self::Search { .. }
			| Self::Usages(_)
			| Self::Change => None,
		}
	}

	fn usage_focus(&self) -> Option<&UsageFocus> {
		match self {
			Self::Usages(focus) => Some(focus),
			Self::None
			| Self::Text { .. }
			| Self::Invalid { .. }
			| Self::Search { .. }
			| Self::Change => None,
		}
	}

	fn error(&self) -> Option<(&str, &str)> {
		match self {
			Self::Invalid { raw, error } => Some((raw, error)),
			Self::None
			| Self::Text { .. }
			| Self::Search { .. }
			| Self::Usages(_)
			| Self::Change => None,
		}
	}

	fn filters_navigator(&self) -> bool {
		matches!(
			self,
			Self::Text { .. } | Self::Search { .. } | Self::Usages(_) | Self::Change
		)
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
	fn new(
		store: MemoryIndexStore,
		scheme: String,
		rules: PathBuf,
		profile: Option<String>,
	) -> Self {
		let registry = FeatureRegistry::static_registry();
		let route = registry.initial_route();
		let nav_count = registry.navigation().len();
		let command_count = registry.commands().len();
		let navigator = build_navigator(&store);
		let change_navigator = build_change_navigator(&store);
		let mut app_store = AppStore::from_memory_store(store);
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
		app.update_shell(|shell| shell.route = route);
		app.set_status(format!(
			"Enter opens nodes, Esc/left closes, / filters, s searches, d changes, u usages, y copies panel, c checks, q quits ({nav_count} nav items, {command_count} commands)"
		));
		app.refresh_results(false);
		app
	}

	fn boot(opts: SessionOptions, scheme: String, rules: PathBuf, profile: Option<String>) -> Self {
		let mut app = Self::new(MemoryIndexStore::empty(opts), scheme, rules, profile);
		app.startup_load_pending = true;
		app.set_status("loading index...");
		app
	}

	fn status(&self) -> &str {
		self.app_store.status()
	}

	fn set_status(&mut self, status: impl Into<String>) {
		self.app_store.set_status(status);
	}

	fn append_status(&mut self, status: impl AsRef<str>) {
		self.app_store.append_status(status);
	}

	fn check_state(&self) -> &CheckState {
		self.app_store.check_state()
	}

	fn set_check_state(&mut self, state: CheckState) {
		self.app_store.set_check_state(state);
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

	fn filter_draft(&self) -> &str {
		&self.app_store.shell().filter_draft
	}

	fn search_draft(&self) -> &str {
		&self.app_store.shell().search_draft
	}

	fn last_panel_width(&self) -> usize {
		self.app_store.shell().last_panel_width
	}

	fn store(&self) -> &MemoryIndexStore {
		self.app_store.index()
	}

	fn store_mut(&mut self) -> &mut MemoryIndexStore {
		self.app_store.index_mut()
	}

	fn replace_store(&mut self, store: MemoryIndexStore) {
		self.app_store.replace_index(store);
	}

	fn update_shell(&mut self, f: impl FnOnce(&mut ShellSlice)) {
		self.app_store.update_shell(f);
	}

	fn selected(&self) -> Option<DefLocation> {
		self.selected_nav_row().and_then(|row| match row.kind {
			NavNodeKind::Def(loc) => Some(loc),
			_ => None,
		})
	}

	fn selected_change_entry(&self) -> Option<&ChangeEntry> {
		self.selected_nav_row().and_then(|row| match row.kind {
			NavNodeKind::Change(idx) => self.store().change_index().entries.get(idx),
			NavNodeKind::Def(loc) => self.store().change_for_def(&loc),
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
			ActiveFilter::Search { hits, .. } => hits.iter().map(|hit| hit.loc).collect(),
			ActiveFilter::Usages(focus) => focus.contexts.clone(),
			ActiveFilter::Change => self.store().changed_defs(),
			ActiveFilter::Invalid { .. } => Vec::new(),
			ActiveFilter::None | ActiveFilter::Text { .. } => self
				.store()
				.all_navigable_defs(self.active_filter().query()),
		}
	}

	fn navigation_scope(&self) -> NavigationScope {
		if self.active_filter().error().is_some() {
			NavigationScope::Invalid
		} else if matches!(self.active_filter(), ActiveFilter::Change) {
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
		if self.mode() == UiMode::EditingFilter {
			return display_filter(self.filter_draft()).to_string();
		}
		if self.mode() == UiMode::EditingSearch {
			return format!("search:{}", display_filter(self.search_draft()));
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
			VisualizationMode::Search if self.active_filter().error().is_some() => View::Tree,
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
		self.update_shell(|shell| {
			shell.view = view;
			shell.panel_policy = policy;
			shell.route = view.route();
		});
	}

	fn scope_label(&self) -> String {
		match self.active_filter() {
			ActiveFilter::None => "all".to_string(),
			ActiveFilter::Text { query, .. } => query.describe(),
			ActiveFilter::Invalid { raw, .. } => format!("invalid {}", display_filter(raw)),
			ActiveFilter::Search { raw, .. } => format!("search:{raw}"),
			ActiveFilter::Usages(focus) => focus.label.clone(),
			ActiveFilter::Change => self.store().change_index().scope.clone(),
		}
	}

	fn focus_usages(&mut self, loc: DefLocation) {
		let focus = self.store().usage_focus(loc);
		let label = focus.label.clone();
		let refs_len = focus.refs.len();
		let contexts_len = focus.contexts.len();
		self.update_shell(|shell| {
			shell.mode = UiMode::Normal;
			shell.filter_draft.clear();
			shell.search_draft.clear();
			shell.view_mode = VisualizationMode::Usages;
			shell.panel_policy = PanelPolicy::Contextual;
			shell.active_filter = ActiveFilter::Usages(focus);
		});
		self.refresh_results(true);
		self.sync_contextual_view();
		self.set_status(format!(
			"usages of {label}: {} reference(s), {} navigable context(s)",
			refs_len, contexts_len
		));
	}

	fn start_filter_edit(&mut self) {
		let draft = self
			.active_filter()
			.text_raw()
			.map(str::to_string)
			.unwrap_or_default();
		self.update_shell(|shell| {
			shell.mode = UiMode::EditingFilter;
			shell.filter_draft = draft;
		});
		self.set_status(
			"type a structural filter, Enter applies, Esc cancels: Resolver, kind:interface, kind:method async.*",
		);
	}

	fn start_search_edit(&mut self) {
		let draft = match self.active_filter() {
			ActiveFilter::Search { raw, .. } => raw.clone(),
			_ => String::new(),
		};
		self.update_shell(|shell| {
			shell.mode = UiMode::EditingSearch;
			shell.search_draft = draft;
		});
		self.set_status(
			"type a symbol search, Enter applies, Esc cancels: customer resolver format",
		);
	}

	fn edit_input(&mut self, edit: FilterEdit) {
		let mut status = String::new();
		self.update_shell(|shell| {
			let (draft, label) = match shell.mode {
				UiMode::EditingSearch => (&mut shell.search_draft, "search"),
				UiMode::EditingFilter | UiMode::Normal => (&mut shell.filter_draft, "filter"),
			};
			match edit {
				FilterEdit::Push(c) => draft.push(c),
				FilterEdit::Backspace => {
					draft.pop();
				}
				FilterEdit::Clear => draft.clear(),
			}
			status = format!("draft {label}: {}", display_filter(draft));
		});
		self.set_status(status);
	}

	fn apply_filter(&mut self) {
		let raw = self.filter_draft().trim().to_string();
		let active_filter = match parse_filter(&raw) {
			Ok(Some(query)) => ActiveFilter::Text {
				raw: raw.clone(),
				query,
			},
			Ok(None) => ActiveFilter::None,
			Err(error) => ActiveFilter::Invalid {
				raw: raw.clone(),
				error: error.to_string(),
			},
		};
		let view_mode = match &active_filter {
			ActiveFilter::None => VisualizationMode::Explorer,
			ActiveFilter::Text { .. } | ActiveFilter::Invalid { .. } => VisualizationMode::Search,
			ActiveFilter::Search { .. } => VisualizationMode::Search,
			ActiveFilter::Usages(_) => VisualizationMode::Usages,
			ActiveFilter::Change => VisualizationMode::Change,
		};
		self.update_shell(|shell| {
			shell.mode = UiMode::Normal;
			shell.active_filter = active_filter;
			shell.view_mode = view_mode;
			shell.panel_policy = PanelPolicy::Contextual;
		});
		self.refresh_results(true);
		self.sync_contextual_view();
		if let Some((raw, _)) = self.active_filter().error() {
			self.set_status(format!("invalid filter regex: /{raw}"));
		} else {
			self.set_status(format!(
				"filter: {} ({}/{})",
				self.filter_label(),
				self.visible_defs().len(),
				self.store().stats().defs
			));
		}
	}

	fn apply_search(&mut self) {
		let raw = self.search_draft().trim().to_string();
		if raw.is_empty() {
			self.clear_filter();
			self.set_status("search cleared");
			return;
		}
		let hits = self.store().search_symbols(&raw, 500);
		let hit_count = hits.len();
		let first_hit = hits.first().map(|hit| hit.loc);
		self.update_shell(|shell| {
			shell.mode = UiMode::Normal;
			shell.active_filter = ActiveFilter::Search {
				raw: raw.clone(),
				hits,
			};
			shell.view_mode = VisualizationMode::Search;
			shell.panel_policy = PanelPolicy::Contextual;
		});
		self.refresh_results(true);
		if let Some(loc) = first_hit {
			self.select_def(loc);
		}
		self.sync_contextual_view();
		self.set_status(format!(
			"search: {raw} ({}/{})",
			hit_count,
			self.store().stats().defs
		));
	}

	fn cancel_input(&mut self) {
		let input = match self.mode() {
			UiMode::EditingSearch => "search",
			UiMode::EditingFilter | UiMode::Normal => "filter",
		};
		self.update_shell(|shell| shell.mode = UiMode::Normal);
		self.set_status(format!(
			"{input} edit canceled; active filter: {}",
			self.filter_label()
		));
	}

	fn clear_filter(&mut self) {
		self.update_shell(|shell| {
			shell.mode = UiMode::Normal;
			shell.view_mode = VisualizationMode::Explorer;
			shell.panel_policy = PanelPolicy::Contextual;
			shell.change_panel = ChangePanelMode::Diff;
			shell.active_filter = ActiveFilter::None;
			shell.filter_draft.clear();
			shell.search_draft.clear();
		});
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
		self.update_shell(|shell| {
			shell.mode = UiMode::Normal;
			shell.filter_draft.clear();
			shell.search_draft.clear();
			shell.view_mode = VisualizationMode::Change;
			shell.panel_policy = PanelPolicy::Contextual;
			shell.change_panel = ChangePanelMode::Diff;
			shell.active_filter = ActiveFilter::Change;
		});
		self.refresh_results(true);
		self.select_first_change();
		self.sync_contextual_view();
		let changes = self.store().change_index();
		self.set_status(format!(
			"changes: {} declaration(s) across {} file(s)",
			changes.entries.len(),
			changes.changed_file_count()
		));
	}

	fn toggle_change_usages(&mut self) {
		let Some(change) = self.selected_change_entry() else {
			self.set_status("select a changed declaration before toggling blast radius");
			return;
		};
		let name = change.name.clone();
		let next_panel = match self.change_panel() {
			ChangePanelMode::Diff => ChangePanelMode::Usages,
			ChangePanelMode::Usages => ChangePanelMode::Diff,
		};
		self.update_shell(|shell| shell.change_panel = next_panel);
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
			StoreEvent::ChangeIndex => {
				runtime::TaskSpec::refresh_change_index(self.store().change_index_refresh_input())
			}
			StoreEvent::FullIndex => runtime::TaskSpec::reload_store(self.store().options()),
		};
		self.queue_task(task)
	}

	fn handle_store_event_sync(&mut self, event: StoreEvent) {
		match event {
			StoreEvent::ChangeIndex => {
				self.store_mut().refresh_change_index();
				self.apply_refreshed_change_store("change index refreshed".to_string());
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
			ActiveFilter::Search { raw, .. } => ActiveFilter::Search {
				raw: raw.clone(),
				hits: self.store().search_symbols(raw, 500),
			},
			ActiveFilter::Usages(focus) => ActiveFilter::Usages(
				self.store()
					.usage_focus_for_target(focus.target.clone(), focus.label.clone()),
			),
			ActiveFilter::None => ActiveFilter::None,
			ActiveFilter::Text { raw, query } => ActiveFilter::Text {
				raw: raw.clone(),
				query: query.clone(),
			},
			ActiveFilter::Invalid { raw, error } => ActiveFilter::Invalid {
				raw: raw.clone(),
				error: error.clone(),
			},
			ActiveFilter::Change => ActiveFilter::Change,
		};
		self.update_shell(|shell| shell.active_filter = active_filter);
	}

	fn move_down(&mut self) {
		let changed = self.dispatch_navigation(NavigationAction::MoveDown);
		if changed {
			self.sync_contextual_view();
		}
	}

	fn move_up(&mut self) {
		let changed = self.dispatch_navigation(NavigationAction::MoveUp);
		if changed {
			self.sync_contextual_view();
		}
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
			TaskOutcome::ChangeIndexRefreshed(store) => {
				self.replace_store(*store);
				self.apply_refreshed_change_store(format!("{} completed", result.label));
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
		let snapshot = active_panel_snapshot(self);
		let component = snapshot.component.as_str().to_string();
		let text = snapshot.to_text(self);
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
		Ok(self.update(AppAction::Ui(Msg::Key(key))))
	}

	fn update(&mut self, action: AppAction) -> bool {
		let action = match action {
			AppAction::TaskCompleted(result) => {
				if self.app_store.complete_task(&result) {
					self.handle_task_result(result);
				} else {
					self.set_status(format!("ignored stale task result: {}", result.label));
				}
				return false;
			}
			action => action,
		};
		let effects = {
			let transition = self.app_store.dispatch(&action);
			transition.take_effects()
		};
		if self.apply_effects(effects) {
			return true;
		}
		match action {
			AppAction::Ui(msg) => self.update_msg(msg),
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

	fn update_msg(&mut self, msg: Msg) -> bool {
		let msg = match msg {
			Msg::Key(key) => key_to_msg(self.mode(), key),
			msg => msg,
		};
		let route = self.route().clone();
		let mut ctx = ScreenContext { route: &route };
		let effects = match Screen::handle_msg(self, msg, &mut ctx) {
			Ok(effects) => effects,
			Err(error) => vec![Effect::Notify(format!("screen error: {error:#}"))],
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
			Effect::Back => {}
			Effect::Quit => return true,
			Effect::Notify(message) => self.set_status(message),
			Effect::Refresh => self.refresh_results(false),
			Effect::Spawn(task) => {
				self.queue_task(task);
			}
			Effect::None => {}
		}
		false
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

	fn navigate(&mut self, route: Route) {
		if !self.registry.can_open(&route) {
			self.set_status(format!("unknown route: {}/{}", route.feature, route.path));
			return;
		}
		if let Some(view) = View::from_route_path(&route.path) {
			self.set_view(view, PanelPolicy::Manual);
			return;
		}
		self.update_shell(|shell| shell.route = route);
	}
}

impl Screen for App {
	fn title(&self) -> String {
		"Explorer".to_string()
	}

	fn component(&self) -> ComponentId {
		ComponentId::PanelOverview
	}

	fn render(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &RenderContext<'_>) {
		let _ = ctx.route;
		render_shell(frame, area, self);
	}

	fn handle_msg(&mut self, msg: Msg, ctx: &mut ScreenContext<'_>) -> anyhow::Result<Vec<Effect>> {
		let _ = ctx.route;
		match msg {
			Msg::Quit => return Ok(vec![Effect::Quit]),
			Msg::CycleView => return Ok(vec![Effect::Navigate(self.view().next().route())]),
			Msg::ShowView(view) => return Ok(vec![Effect::Navigate(view.route())]),
			Msg::StartFilterEdit => self.start_filter_edit(),
			Msg::StartSearchEdit => self.start_search_edit(),
			Msg::FilterInput(edit) => self.edit_input(edit),
			Msg::ApplyFilter => self.apply_filter(),
			Msg::ApplySearch => self.apply_search(),
			Msg::CancelInput => self.cancel_input(),
			Msg::ClearFilter => self.clear_filter(),
			Msg::FocusUsages => {
				let had_selection = self.selected().is_some();
				self.focus_usages_of_selected();
				if had_selection || self.view_mode() == VisualizationMode::Change {
					return Ok(Vec::new());
				}
			}
			Msg::ToggleChangeMode => {
				self.toggle_change_mode();
				return Ok(Vec::new());
			}
			Msg::CopyPanelSnapshot => {
				self.copy_panel_snapshot();
				return Ok(Vec::new());
			}
			Msg::RunCheck => {
				self.run_check();
				return Ok(Vec::new());
			}
			Msg::MoveDown => self.move_down(),
			Msg::MoveUp => self.move_up(),
			Msg::Home => {
				let changed = self.dispatch_navigation(NavigationAction::Home);
				if changed {
					self.sync_contextual_view();
				}
			}
			Msg::End => {
				let changed = self.dispatch_navigation(NavigationAction::End);
				if changed {
					self.sync_contextual_view();
				}
			}
			Msg::ToggleNode => self.toggle_selected_nav(),
			Msg::OpenNode => self.open_selected_nav(),
			Msg::CloseNode => {
				if !self.close_selected_nav() && self.has_clearable_scope() {
					self.clear_filter();
				}
			}
			Msg::Help => {
				self.set_status(
					"keys: Enter/right open, Esc/left close, / filter, s search, d changes, u usages, y copy panel, x clear, Tab/1-5 panels, c check, q quit",
				);
			}
			Msg::Key(_) | Msg::Noop => {}
		}
		Ok(Vec::new())
	}
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &mut App) {
	let _title = Screen::title(app);
	let _component = Screen::component(app);
	let route = app.route().clone();
	let ctx = RenderContext { route: &route };
	Screen::render(app, frame, frame.area(), &ctx);
}

fn render_shell(frame: &mut ratatui::Frame<'_>, area: Rect, app: &mut App) {
	let rows = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(1),
			Constraint::Min(0),
			Constraint::Length(1),
		])
		.split(area);
	render_header(frame, rows[0], app);
	render_body(frame, rows[1], app);
	render_footer(frame, rows[2], app);
}

fn render_header(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	frame.render_widget(
		Paragraph::new(header_line(app, usize::from(area.width))),
		area,
	);
}

fn header_line(app: &App, width: usize) -> Line<'static> {
	let mode = app.view_mode().label();
	let prefix_width = visible_len("code-moniker ")
		+ visible_len(ComponentId::Header.as_str())
		+ 2 + visible_len(" mode ")
		+ visible_len(mode)
		+ visible_len("  scope ");
	let scope = fit_text(
		&app.scope_label(),
		width.saturating_sub(prefix_width),
		FitMode::Middle,
	);
	Line::from(vec![
		Span::styled(
			"code-moniker ",
			Style::default()
				.fg(THEME.brand)
				.add_modifier(Modifier::BOLD),
		),
		marker(ComponentId::Header),
		Span::raw(" "),
		Span::raw("mode "),
		Span::styled(
			app.view_mode().label(),
			Style::default()
				.fg(THEME.section)
				.add_modifier(Modifier::BOLD),
		),
		Span::raw("  scope "),
		Span::styled(scope, Style::default().fg(THEME.nav.symbol)),
	])
}

fn render_body(frame: &mut ratatui::Frame<'_>, area: Rect, app: &mut App) {
	let cols = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
		.split(area);
	app.update_shell(|shell| shell.last_panel_width = panel_content_width(cols[1]));
	render_left_pane(frame, cols[0], app);
	match app.view() {
		View::Overview => render_overview(frame, cols[1], app),
		View::Tree => render_outline(frame, cols[1], app),
		View::Refs => render_refs(frame, cols[1], app),
		View::Check => render_check(frame, cols[1], app),
		View::Change => render_change(frame, cols[1], app),
	}
}

fn render_left_pane(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	if search_input_visible(app) && area.height >= 5 {
		let rows = Layout::default()
			.direction(Direction::Vertical)
			.constraints([Constraint::Length(3), Constraint::Min(0)])
			.split(area);
		render_search_input(frame, rows[0], app);
		render_nav_list(frame, rows[1], app);
	} else {
		render_nav_list(frame, area, app);
	}
}

fn search_input_visible(app: &App) -> bool {
	app.mode() == UiMode::EditingSearch
		|| matches!(app.active_filter(), ActiveFilter::Search { .. })
}

fn search_input_value(app: &App) -> String {
	if app.mode() == UiMode::EditingSearch {
		return app.search_draft().to_string();
	}
	match app.active_filter() {
		ActiveFilter::Search { raw, .. } => raw.clone(),
		_ => String::new(),
	}
}

fn search_input_title(app: &App) -> String {
	if app.mode() == UiMode::EditingSearch {
		"search focused".to_string()
	} else {
		"search".to_string()
	}
}

fn render_search_input(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let focused = app.mode() == UiMode::EditingSearch;
	let value = search_input_value(app);
	let width = panel_content_width(area);
	let prompt = if focused { "> " } else { "  " };
	let hint = if focused {
		"  Enter apply  Esc cancel"
	} else {
		"  s edit  x clear"
	};
	let value_width = width
		.saturating_sub(visible_len(prompt))
		.saturating_sub(visible_len(hint));
	let displayed_value = fit_text(display_filter(&value), value_width, FitMode::Middle);
	let line = Line::from(vec![
		Span::styled(prompt, Style::default().fg(THEME.nav.marker)),
		Span::styled(
			displayed_value.clone(),
			Style::default()
				.fg(THEME.nav.symbol)
				.add_modifier(if focused {
					Modifier::BOLD
				} else {
					Modifier::empty()
				}),
		),
		Span::styled(hint, Style::default().fg(THEME.nav.meta)),
	]);
	let border_style = if focused {
		Style::default().fg(THEME.status_label)
	} else {
		Style::default().fg(THEME.component_marker)
	};
	let input = Paragraph::new(line).block(
		Block::default()
			.title(block_title(
				search_input_title(app),
				ComponentId::SearchInput,
			))
			.border_style(border_style)
			.borders(Borders::ALL),
	);
	frame.render_widget(input, area);
	if focused {
		let cursor_offset = visible_len(prompt) + visible_len(&displayed_value);
		let max_x = area.x.saturating_add(area.width.saturating_sub(2));
		let x = area
			.x
			.saturating_add(1)
			.saturating_add(cursor_offset as u16)
			.min(max_x);
		frame.set_cursor_position(Position {
			x,
			y: area.y.saturating_add(1),
		});
	}
}

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let prefix = match app.mode() {
		UiMode::EditingFilter => "filter",
		UiMode::EditingSearch => "search",
		UiMode::Normal => "status",
	};
	let line = Line::from(vec![
		Span::styled(
			format!("{prefix}: "),
			Style::default().fg(THEME.status_label),
		),
		marker(ComponentId::Status),
		Span::raw(" "),
		Span::raw(app.status()),
	]);
	frame.render_widget(Paragraph::new(line), area);
}

fn render_nav_list(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let visible_rows = area.height.saturating_sub(2) as usize;
	let nav_rows = app.nav_rows();
	let selection = app.selected_nav_index();
	let start = if visible_rows == 0 {
		0
	} else {
		selection.saturating_sub(visible_rows.saturating_sub(1))
	};
	let end = (start + visible_rows).min(nav_rows.len());
	let items: Vec<ListItem<'_>> = nav_rows[start..end]
		.iter()
		.enumerate()
		.map(|(offset, row)| {
			let idx = start + offset;
			let line = nav_row_line(app, row, idx == selection);
			let style = if idx == selection {
				Style::default().bg(THEME.nav.selected_bg)
			} else {
				Style::default()
			};
			ListItem::new(line).style(style)
		})
		.collect();
	let title = if app.is_filtered() {
		if app.view_mode() == VisualizationMode::Change {
			format!(
				" change {} files {} defs ",
				matched_file_count(app.visible_defs()),
				app.visible_defs().len()
			)
		} else {
			format!(
				" filtered {} files {} defs ",
				matched_file_count(app.visible_defs()),
				app.visible_defs().len()
			)
		}
	} else if app.active_filter().error().is_some() {
		" filtered invalid ".to_string()
	} else {
		format!(
			" navigator {} files {} defs ",
			app.store().stats().files,
			app.app_store.navigation().explorer_def_count()
		)
	};
	let list = List::new(items).block(
		Block::default()
			.title(block_title(title, ComponentId::Navigator))
			.borders(Borders::ALL),
	);
	frame.render_widget(list, area);
}

fn nav_row_line(app: &App, row: &NavRow, selected: bool) -> Line<'static> {
	let marker = if selected { ">" } else { " " };
	let indent = "  ".repeat(row.depth);
	let twisty = if row.has_children {
		if app.active_expanded().contains(&row.key) {
			"▾"
		} else {
			"▸"
		}
	} else {
		" "
	};
	let mut spans = vec![
		Span::styled(marker, Style::default().fg(THEME.nav.marker)),
		Span::raw(" "),
		Span::raw(indent),
		Span::styled(twisty, Style::default().fg(THEME.nav.twisty)),
		Span::raw(" "),
	];
	match row.kind {
		NavNodeKind::Lang => {
			let label = if row.has_children {
				format!("{}/", row.label)
			} else {
				row.label.clone()
			};
			spans.push(Span::styled(
				label,
				Style::default()
					.fg(THEME.nav.language)
					.add_modifier(Modifier::BOLD),
			));
			spans.push(nav_count_span(row));
		}
		NavNodeKind::Dir => {
			spans.push(Span::styled(
				format!("{}/", row.label),
				Style::default().fg(THEME.nav.directory),
			));
			spans.push(nav_count_span(row));
		}
		NavNodeKind::File(_) | NavNodeKind::ChangeFile => {
			spans.push(Span::styled(
				row.label.clone(),
				Style::default()
					.fg(THEME.nav.file)
					.add_modifier(Modifier::BOLD),
			));
			spans.push(nav_count_span(row));
			if let Some(count) = row_change_count(app, row) {
				spans.push(change_count_span(count));
			}
		}
		NavNodeKind::Def(loc) => {
			let def = app.store().def(&loc);
			let kind = def_kind(def);
			let group = definition_kind_group(app.store().file(loc.file).lang, &kind);
			spans.push(Span::styled(
				kind.clone(),
				Style::default().fg(THEME.kind.color_for_group(group)),
			));
			spans.push(Span::raw(" "));
			spans.push(Span::styled(
				row.label.clone(),
				Style::default().fg(THEME.nav.symbol),
			));
			if row.def_count > 1 {
				spans.push(Span::styled(
					format!("  {} children", row.def_count - 1),
					Style::default().fg(THEME.nav.meta),
				));
			}
			if let Some(change) = app.store().change_for_def(&loc) {
				spans.push(Span::raw("  "));
				spans.push(change_marker_span(change.status));
				let usages = app.store().change_usage_refs(change).len();
				spans.push(Span::styled(
					format!("  {usages} usages"),
					Style::default().fg(THEME.nav.meta),
				));
			}
		}
		NavNodeKind::Change(idx) => {
			let change = &app.store().change_index().entries[idx];
			let group = definition_kind_group(change.lang, &change.kind);
			spans.push(Span::styled(
				change.kind.clone(),
				Style::default().fg(THEME.kind.color_for_group(group)),
			));
			spans.push(Span::raw(" "));
			spans.push(Span::styled(
				row.label.clone(),
				Style::default().fg(THEME.nav.symbol),
			));
			spans.push(Span::raw("  "));
			spans.push(change_marker_span(change.status));
			let usages = app.store().change_usage_refs(change).len();
			spans.push(Span::styled(
				format!("  {usages} usages"),
				Style::default().fg(THEME.nav.meta),
			));
		}
		NavNodeKind::Root => {}
	}
	Line::from(spans)
}

fn nav_count_span(row: &NavRow) -> Span<'static> {
	let label = match (row.file_count, row.def_count) {
		(0, defs) => format!("  {defs} defs"),
		(files, defs) => format!("  {files} files  {defs} defs"),
	};
	Span::styled(label, Style::default().fg(THEME.nav.meta))
}

fn row_change_count(app: &App, row: &NavRow) -> Option<usize> {
	let NavNodeKind::File(file_idx) = row.kind else {
		return None;
	};
	let count = app.store().change_count_for_file(file_idx);
	(count > 0).then_some(count)
}

fn change_count_span(count: usize) -> Span<'static> {
	Span::styled(
		format!("  {count} change(s)"),
		Style::default().fg(THEME.change_modified),
	)
}

fn change_marker_span(status: ChangeStatus) -> Span<'static> {
	Span::styled(
		status.marker().to_string(),
		Style::default().fg(change_status_color(status)),
	)
}

fn change_status_color(status: ChangeStatus) -> ratatui::style::Color {
	match status {
		ChangeStatus::Added => THEME.change_added,
		ChangeStatus::Modified => THEME.change_modified,
		ChangeStatus::Removed => THEME.danger,
	}
}

fn matched_file_count(defs: &[DefLocation]) -> usize {
	defs.iter()
		.map(|loc| loc.file)
		.collect::<BTreeSet<_>>()
		.len()
}

struct PanelSnapshot {
	title: &'static str,
	component: ComponentId,
	lines: Vec<Line<'static>>,
}

impl PanelSnapshot {
	fn to_text(&self, app: &App) -> String {
		let mut lines = vec![
			"code-moniker panel snapshot".to_string(),
			format!("component {}", self.component.as_str()),
			format!("title     {}", self.title),
			format!("mode      {}", app.view_mode().label()),
			format!("scope     {}", app.scope_label()),
			String::new(),
		];
		lines.extend(self.lines.iter().map(plain_line_text));
		lines.join("\n")
	}
}

fn active_panel_snapshot(app: &App) -> PanelSnapshot {
	let width = app.last_panel_width();
	match app.view() {
		View::Overview => PanelSnapshot {
			title: "overview",
			component: ComponentId::PanelOverview,
			lines: overview_lines(app, width),
		},
		View::Tree => PanelSnapshot {
			title: "outline",
			component: ComponentId::PanelOutline,
			lines: outline_panel_lines(app, width),
		},
		View::Refs => refs_panel_snapshot(app, width),
		View::Check => PanelSnapshot {
			title: "check",
			component: ComponentId::PanelCheck,
			lines: check_panel_lines(app, width),
		},
		View::Change => PanelSnapshot {
			title: "change",
			component: ComponentId::PanelChange,
			lines: change_panel_lines(app, width),
		},
	}
}

fn refs_panel_snapshot(app: &App, width: usize) -> PanelSnapshot {
	if let Some(focus) = app.active_filter().usage_focus() {
		return PanelSnapshot {
			title: "usages",
			component: ComponentId::PanelUsages,
			lines: usage_focus_lines(app, focus, width),
		};
	}
	let lines = match app.selected() {
		Some(loc) => {
			let def = app.store().def(&loc);
			refs_panel_lines(app, loc, def, width)
		}
		None => vec![panel::muted("select a declaration to inspect refs")],
	};
	PanelSnapshot {
		title: "refs",
		component: ComponentId::PanelRefs,
		lines,
	}
}

fn plain_line_text(line: &Line<'_>) -> String {
	line.spans
		.iter()
		.map(|span| span.content.as_ref())
		.collect()
}

fn overview_lines(app: &App, width: usize) -> Vec<Line<'static>> {
	let stats = app.store().stats();
	let total_ms = stats.scan_ms + stats.extract_ms + stats.index_ms;
	let mut lines = vec![
		panel::section("summary"),
		detail_line("root", &app.store().root(), width, FitMode::Tail),
		detail_line("files", &stats.files.to_string(), width, FitMode::Tail),
		detail_line("defs", &stats.defs.to_string(), width, FitMode::Tail),
		detail_line("refs", &stats.refs.to_string(), width, FitMode::Tail),
		detail_line("time", &format!("{total_ms} ms"), width, FitMode::Tail),
		detail_line(
			"scan",
			&format!("{} ms", stats.scan_ms),
			width,
			FitMode::Tail,
		),
		detail_line(
			"extract",
			&format!("{} ms", stats.extract_ms),
			width,
			FitMode::Tail,
		),
		detail_line(
			"index",
			&format!("{} ms", stats.index_ms),
			width,
			FitMode::Tail,
		),
		panel::blank(),
		panel::section("languages"),
	];
	let language_columns = [
		Column::left("lang", 10),
		Column::right("files", 7),
		Column::right("defs", 8),
		Column::right("refs", 8),
	];
	lines.push(panel::table_header(&language_columns, width));
	lines.push(panel::separator(panel::table_width(
		&language_columns,
		width,
	)));
	for (lang, totals) in &stats.by_lang {
		lines.push(panel::table_row(
			&language_columns,
			&[
				lang.to_string(),
				totals.files.to_string(),
				totals.defs.to_string(),
				totals.refs.to_string(),
			],
			width,
		));
	}
	lines.push(panel::blank());
	lines.push(panel::section("shapes"));
	let shape_columns = [Column::left("shape", 12), Column::right("count", 8)];
	lines.push(panel::table_header(&shape_columns, width));
	lines.push(panel::separator(panel::table_width(&shape_columns, width)));
	for (shape, count) in &stats.by_shape {
		lines.push(panel::table_row(
			&shape_columns,
			&[shape.to_string(), count.to_string()],
			width,
		));
	}
	lines
}

fn render_overview(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	render_panel(
		frame,
		area,
		"overview",
		ComponentId::PanelOverview,
		overview_lines(app, panel_content_width(area)),
	);
}

fn outline_panel_lines(app: &App, width: usize) -> Vec<Line<'static>> {
	let Some(loc) = app.selected() else {
		return nav_selection_lines(app, width);
	};
	let file = app.store().file(loc.file);
	let def = app.store().def(&loc);
	let mut lines = vec![
		panel::section("selected"),
		detail_line("kind", &def_kind(def), width, FitMode::Tail),
		detail_line("name", &last_name(&def.moniker), width, FitMode::Middle),
		detail_line(
			"file",
			&file.rel_path.display().to_string(),
			width,
			FitMode::Tail,
		),
		detail_line(
			"moniker",
			&compact_moniker(&def.moniker),
			width,
			FitMode::Middle,
		),
	];
	if let Some(change) = app.store().change_for_def(&loc) {
		lines.push(Line::raw(""));
		lines.extend(change_summary_lines(app, loc, change, width));
	}
	lines.extend([panel::blank(), panel::section("children")]);
	let children = app.store().children_by_parent(&def.moniker);
	if children.is_empty() {
		lines.push(panel::muted("none"));
	} else {
		let child_columns = [Column::left("kind", 12), Column::left("name", 40)];
		lines.push(panel::table_header(&child_columns, width));
		lines.push(panel::separator(panel::table_width(&child_columns, width)));
		for child in children.iter().take(40) {
			let child_def = app.store().def(child);
			lines.push(panel::table_row(
				&child_columns,
				&[def_kind(child_def), last_name(&child_def.moniker)],
				width,
			));
		}
		if children.len() > 40 {
			lines.push(panel::muted(format!("... {} more", children.len() - 40)));
		}
	}
	lines.push(panel::blank());
	lines.push(Line::from(vec![
		Span::styled(
			"source",
			Style::default()
				.fg(THEME.panel.section)
				.add_modifier(Modifier::BOLD),
		),
		Span::raw(" "),
		marker(ComponentId::SourceSnippet),
	]));
	let snippet = source_snippet_lines(app, &loc, 3);
	if snippet.is_empty() {
		lines.push(panel::muted("no source position"));
	} else {
		lines.extend(snippet);
	}
	lines
}

fn render_outline(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	render_panel_unwrapped(
		frame,
		area,
		"outline",
		ComponentId::PanelOutline,
		outline_panel_lines(app, panel_content_width(area)),
	);
}

fn nav_selection_lines(app: &App, width: usize) -> Vec<Line<'static>> {
	let Some(row) = app.selected_nav_row() else {
		return if let Some((raw, error)) = app.active_filter().error() {
			vec![
				panel::danger_section("invalid filter"),
				detail_line("query", raw, width, FitMode::Tail),
				Line::styled(error.to_string(), Style::default().fg(THEME.danger)),
				panel::blank(),
				panel::section("examples"),
				panel::bullet("Resolver"),
				panel::bullet("kind:interface Resolver"),
				panel::bullet("kind:method ^async"),
			]
		} else if app.is_filtered() {
			vec![
				panel::section("filtered navigator"),
				detail_line("filter", &app.filter_label(), width, FitMode::Tail),
				detail_line("matches", "0", width, FitMode::Tail),
				panel::blank(),
				panel::muted("x clears the filter"),
			]
		} else {
			vec![panel::muted("navigator is empty")]
		};
	};
	let kind = match row.kind {
		NavNodeKind::Root => "root",
		NavNodeKind::Lang => "language",
		NavNodeKind::Dir => "directory",
		NavNodeKind::File(_) | NavNodeKind::ChangeFile => "file",
		NavNodeKind::Def(_) => "declaration",
		NavNodeKind::Change(_) => "change",
	};
	let mut lines = vec![
		panel::section("navigator"),
		detail_line("kind", kind, width, FitMode::Tail),
		detail_line("name", &row.label, width, FitMode::Middle),
		detail_line("files", &row.file_count.to_string(), width, FitMode::Tail),
		detail_line("defs", &row.def_count.to_string(), width, FitMode::Tail),
		panel::blank(),
	];
	if row.has_children {
		let state = if app.active_expanded().contains(&row.key) {
			"opened"
		} else {
			"closed"
		};
		lines.push(detail_line("state", state, width, FitMode::Tail));
		lines.push(panel::muted("Enter toggles, right opens, left closes"));
	} else {
		lines.push(panel::muted("no child node"));
	}
	lines
}

fn render_refs(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let width = panel_content_width(area);
	if let Some(focus) = app.active_filter().usage_focus() {
		render_usage_focus(frame, area, app, focus, width);
		return;
	}
	let Some(loc) = app.selected() else {
		render_panel(
			frame,
			area,
			"refs",
			ComponentId::PanelRefs,
			vec![panel::muted("select a declaration to inspect refs")],
		);
		return;
	};
	let def = app.store().def(&loc);
	render_panel(
		frame,
		area,
		"refs",
		ComponentId::PanelRefs,
		refs_panel_lines(app, loc, def, width),
	);
}

fn render_usage_focus(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	app: &App,
	focus: &UsageFocus,
	width: usize,
) {
	render_panel(
		frame,
		area,
		"usages",
		ComponentId::PanelUsages,
		usage_focus_lines(app, focus, width),
	);
}

fn render_change(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	render_panel(
		frame,
		area,
		"change",
		ComponentId::PanelChange,
		change_panel_lines(app, panel_content_width(area)),
	);
}

fn refs_panel_lines(
	app: &App,
	loc: DefLocation,
	def: &DefRecord,
	width: usize,
) -> Vec<Line<'static>> {
	let file = app.store().file(loc.file);
	let outgoing = app.store().outgoing_refs(&def.moniker);
	let incoming = app.store().incoming_refs(&def.moniker);
	let mut lines = vec![
		panel::section("selected"),
		detail_line("kind", &def_kind(def), width, FitMode::Tail),
		detail_line("name", &last_name(&def.moniker), width, FitMode::Middle),
		detail_line(
			"file",
			&file.rel_path.display().to_string(),
			width,
			FitMode::Tail,
		),
		detail_line(
			"moniker",
			&compact_moniker(&def.moniker),
			width,
			FitMode::Middle,
		),
		panel::blank(),
		panel::section("incoming impact"),
		panel::muted(reference_summary(app, incoming)),
	];
	push_ref_rows(&mut lines, app, incoming, RefDirection::Incoming, 30, width);
	lines.push(panel::blank());
	lines.push(panel::section("outgoing dependencies"));
	lines.push(panel::muted(reference_summary(app, outgoing)));
	push_ref_rows(&mut lines, app, outgoing, RefDirection::Outgoing, 30, width);
	lines
}

fn change_panel_lines(app: &App, width: usize) -> Vec<Line<'static>> {
	let Some(change) = app.selected_change_entry() else {
		return change_overview_lines(app, width);
	};
	match app.change_panel() {
		ChangePanelMode::Diff => change_diff_lines(app, change, width),
		ChangePanelMode::Usages => change_usage_lines(app, change, width),
	}
}

fn change_overview_lines(app: &App, width: usize) -> Vec<Line<'static>> {
	let changes = app.store().change_index();
	let mut lines = vec![
		panel::section("change scope"),
		detail_line("scope", &changes.scope, width, FitMode::Tail),
		detail_line(
			"changes",
			&changes.entries.len().to_string(),
			width,
			FitMode::Tail,
		),
		detail_line(
			"files",
			&changes.changed_file_count().to_string(),
			width,
			FitMode::Tail,
		),
		panel::blank(),
		panel::section("git resources"),
	];
	if changes.resources.is_empty() {
		lines.push(panel::muted("none"));
	} else {
		for resource in &changes.resources {
			let status = if resource.available() {
				"git"
			} else {
				"no git"
			};
			lines.push(detail_line(
				status,
				&format!("{}: {}", resource.label, resource.message),
				width,
				FitMode::Middle,
			));
		}
	}
	if !changes.diagnostics.is_empty() {
		lines.push(panel::blank());
		lines.push(Line::styled(
			"diagnostics",
			Style::default().fg(THEME.danger),
		));
		for diagnostic in &changes.diagnostics {
			lines.push(panel::bullet(diagnostic.clone()));
		}
	}
	lines
}

fn change_diff_lines(app: &App, change: &ChangeEntry, width: usize) -> Vec<Line<'static>> {
	let refs = change_blast_radius_refs_for_change(app, change);
	let mut lines = vec![
		panel::section("changed symbol"),
		detail_line("status", change.status.label(), width, FitMode::Tail),
		detail_line("kind", &change.kind, width, FitMode::Tail),
		detail_line("symbol", &change.name, width, FitMode::Middle),
		detail_line(
			"file",
			&change.file_path.display().to_string(),
			width,
			FitMode::Tail,
		),
		detail_line(
			"moniker",
			&compact_moniker(&change.moniker),
			width,
			FitMode::Middle,
		),
	];
	if let Some((start, end)) = change.line_range {
		let range = if start == end {
			format!("L{start}")
		} else {
			format!("L{start}-L{end}")
		};
		lines.push(detail_line("range", &range, width, FitMode::Tail));
	}
	lines.push(detail_line(
		"hunks",
		&change.hunk_count.to_string(),
		width,
		FitMode::Tail,
	));
	lines.push(panel::blank());
	lines.extend(change_blast_radius_summary(&refs, width));
	lines.push(panel::blank());
	lines.push(panel::muted("u toggles blast radius details"));
	lines
}

fn change_summary_lines(
	app: &App,
	loc: DefLocation,
	change: &ChangeEntry,
	width: usize,
) -> Vec<Line<'static>> {
	let usages = change_blast_radius_refs(app, loc).len();
	vec![
		panel::section("change"),
		detail_line("status", change.status.label(), width, FitMode::Tail),
		detail_line(
			"scope",
			&app.store().change_index().scope,
			width,
			FitMode::Tail,
		),
		detail_line("usages", &usages.to_string(), width, FitMode::Tail),
	]
}

fn change_blast_radius_summary(refs: &[RefLocation], width: usize) -> Vec<Line<'static>> {
	let contexts = refs
		.iter()
		.map(|loc| (loc.file, app_ref_source_index(loc)))
		.collect::<BTreeSet<_>>()
		.len();
	vec![
		panel::section("blast radius"),
		detail_line(
			"direct",
			&format!("{} direct usage(s)", refs.len()),
			width,
			FitMode::Tail,
		),
		detail_line("contexts", &contexts.to_string(), width, FitMode::Tail),
	]
}

fn change_usage_lines(app: &App, change: &ChangeEntry, width: usize) -> Vec<Line<'static>> {
	let refs = change_blast_radius_refs_for_change(app, change);
	let mut lines = change_blast_radius_summary(&refs, width);
	lines.push(panel::blank());
	lines.push(panel::section("references"));
	if refs.is_empty() {
		lines.push(panel::muted("none"));
	} else {
		push_ref_rows(&mut lines, app, &refs, RefDirection::Incoming, 40, width);
	}
	lines
}

fn change_blast_radius_refs(app: &App, loc: DefLocation) -> &[RefLocation] {
	app.store()
		.change_for_def(&loc)
		.map(|change| app.store().change_usage_refs(change))
		.unwrap_or(&[])
}

fn change_blast_radius_refs_for_change<'a>(
	app: &'a App,
	change: &ChangeEntry,
) -> &'a [RefLocation] {
	app.store().change_usage_refs(change)
}

fn app_ref_source_index(loc: &RefLocation) -> usize {
	loc.reference
}

fn usage_focus_lines(app: &App, focus: &UsageFocus, width: usize) -> Vec<Line<'static>> {
	let mut lines = vec![
		panel::section("usage focus"),
		detail_line("symbol", &focus.label, width, FitMode::Middle),
		detail_line(
			"moniker",
			&compact_moniker(&focus.target),
			width,
			FitMode::Middle,
		),
		detail_line("refs", &focus.refs.len().to_string(), width, FitMode::Tail),
		detail_line(
			"contexts",
			&focus.contexts.len().to_string(),
			width,
			FitMode::Tail,
		),
		panel::blank(),
		panel::section("references"),
	];
	if focus.refs.is_empty() {
		lines.push(panel::muted("none"));
	} else {
		push_ref_rows(
			&mut lines,
			app,
			&focus.refs,
			RefDirection::Incoming,
			40,
			width,
		);
	}
	lines
}

fn check_panel_lines(app: &App, width: usize) -> Vec<Line<'static>> {
	match app.check_state() {
		CheckState::Pending => vec![
			panel::section("check"),
			panel::muted("press c to run .code-moniker.toml rules on the loaded graph"),
			detail_line(
				"rules",
				&app.rules.display().to_string(),
				width,
				FitMode::Tail,
			),
			detail_line(
				"profile",
				app.profile.as_deref().unwrap_or("<none>"),
				width,
				FitMode::Tail,
			),
		],
		CheckState::Ready(summary) => vec![
			panel::section("check summary"),
			detail_line(
				"files",
				&summary.files_scanned.to_string(),
				width,
				FitMode::Tail,
			),
			detail_line(
				"flagged",
				&summary.files_with_violations.to_string(),
				width,
				FitMode::Tail,
			),
			detail_line(
				"violations",
				&summary.total_violations.to_string(),
				width,
				FitMode::Tail,
			),
		],
		CheckState::Error(error) => vec![
			Line::styled(
				"check failed",
				Style::default()
					.fg(THEME.danger)
					.add_modifier(Modifier::BOLD),
			),
			panel::bullet(error.clone()),
		],
	}
}

fn render_check(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	render_panel(
		frame,
		area,
		"check",
		ComponentId::PanelCheck,
		check_panel_lines(app, panel_content_width(area)),
	);
}

fn render_panel(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	title: &str,
	component: ComponentId,
	lines: Vec<Line<'_>>,
) {
	let paragraph = Paragraph::new(Text::from(lines))
		.block(
			Block::default()
				.title(block_title(title, component))
				.borders(Borders::ALL),
		)
		.wrap(Wrap { trim: false });
	frame.render_widget(paragraph, area);
}

fn render_panel_unwrapped(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	title: &str,
	component: ComponentId,
	lines: Vec<Line<'_>>,
) {
	let paragraph = Paragraph::new(Text::from(lines)).block(
		Block::default()
			.title(block_title(title, component))
			.borders(Borders::ALL),
	);
	frame.render_widget(paragraph, area);
}

fn panel_content_width(area: Rect) -> usize {
	usize::from(area.width.saturating_sub(2)).max(20)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum RefDirection {
	Incoming,
	Outgoing,
}

fn reference_summary(app: &App, refs: &[RefLocation]) -> String {
	let files = refs
		.iter()
		.map(|loc| app.store().file(loc.file).rel_path.as_path())
		.collect::<BTreeSet<_>>()
		.len();
	match (refs.len(), files) {
		(0, _) => "0 reference(s)".to_string(),
		(count, 1) => format!("{count} reference(s) from 1 file"),
		(count, files) => format!("{count} reference(s) from {files} files"),
	}
}

fn push_ref_rows(
	lines: &mut Vec<Line<'static>>,
	app: &App,
	refs: &[RefLocation],
	direction: RefDirection,
	limit: usize,
	width: usize,
) {
	if refs.is_empty() {
		lines.push(panel::muted("none"));
		return;
	}
	let groups = ref_groups(app, refs, direction);
	for (idx, group) in groups.iter().take(limit).enumerate() {
		if idx > 0 {
			lines.push(panel::blank());
		}
		lines.extend(ref_group_lines(group, width));
	}
	if groups.len() > limit {
		lines.push(panel::muted(format!("... {} more", groups.len() - limit)));
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RefGroup {
	kinds: Vec<String>,
	actor: String,
	location: String,
	endpoint_label: &'static str,
	endpoint: String,
	confidence: String,
	receiver: Option<String>,
	alias: Option<String>,
}

impl RefGroup {
	fn same_context(&self, other: &Self) -> bool {
		self.actor == other.actor
			&& self.location == other.location
			&& self.endpoint_label == other.endpoint_label
			&& self.endpoint == other.endpoint
			&& self.confidence == other.confidence
			&& self.receiver == other.receiver
			&& self.alias == other.alias
	}
}

fn ref_groups(app: &App, refs: &[RefLocation], direction: RefDirection) -> Vec<RefGroup> {
	let mut groups: Vec<RefGroup> = Vec::new();
	for loc in refs {
		let group = ref_group(app, loc, direction);
		if let Some(existing) = groups
			.iter_mut()
			.find(|existing| existing.same_context(&group))
		{
			for kind in group.kinds {
				if !existing.kinds.contains(&kind) {
					existing.kinds.push(kind);
				}
			}
		} else {
			groups.push(group);
		}
	}
	for group in &mut groups {
		sort_reference_kinds(&mut group.kinds);
	}
	groups
}

fn ref_group(app: &App, loc: &RefLocation, direction: RefDirection) -> RefGroup {
	let file = app.store().file(loc.file);
	let reference = app.store().reference(loc);
	let source = file.graph.def_at(reference.source);
	let kind = ref_kind(reference);
	let actor = match direction {
		RefDirection::Incoming => last_name(&source.moniker),
		RefDirection::Outgoing => last_name(&reference.target),
	};
	let endpoint_label = match direction {
		RefDirection::Incoming => "source",
		RefDirection::Outgoing => "target",
	};
	let endpoint = match direction {
		RefDirection::Incoming => compact_moniker(&source.moniker),
		RefDirection::Outgoing => compact_moniker(&reference.target),
	};
	RefGroup {
		kinds: vec![kind],
		actor,
		location: ref_location(app, loc),
		endpoint_label,
		endpoint,
		confidence: ref_confidence(reference),
		receiver: ref_attr(&reference.receiver_hint).map(str::to_string),
		alias: ref_attr(&reference.alias).map(str::to_string),
	}
}

fn ref_group_lines(group: &RefGroup, width: usize) -> Vec<Line<'static>> {
	let mut lines = vec![
		ref_actor_line(&group.actor, &group.confidence, width),
		ref_kinds_line(&group.kinds, width),
		ref_location_line(&group.location, width),
		ref_endpoint_line(group.endpoint_label, &group.endpoint, width),
	];
	if let Some(attrs) = ref_attrs_line(group, width) {
		lines.push(attrs);
	}
	lines
}

fn ref_actor_line(actor: &str, confidence: &str, width: usize) -> Line<'static> {
	let prefix = "  ";
	let suffix = if confidence == "-" {
		String::new()
	} else {
		format!("  {confidence}")
	};
	let actor_width = width.saturating_sub(visible_len(&prefix) + visible_len(&suffix));
	Line::from(vec![
		Span::raw("  "),
		Span::styled(
			fit_text(actor, actor_width, FitMode::Middle),
			Style::default().fg(THEME.nav.symbol),
		),
		Span::styled(suffix, Style::default().fg(THEME.nav.meta)),
	])
}

fn ref_kinds_line(kinds: &[String], width: usize) -> Line<'static> {
	let prefix = "    kinds  ";
	let value = kinds.join(", ");
	let value_width = width.saturating_sub(visible_len(prefix));
	let color = kinds
		.first()
		.map(|kind| THEME.kind.color_for_group(reference_kind_group(kind)))
		.unwrap_or(THEME.kind.fallback);
	Line::from(vec![
		Span::raw("    "),
		Span::styled("kinds  ", Style::default().fg(THEME.nav.meta)),
		Span::styled(
			fit_text(&value, value_width, FitMode::Middle),
			Style::default().fg(color),
		),
	])
}

fn ref_location_line(location: &str, width: usize) -> Line<'static> {
	let prefix = "    at ";
	let value_width = width.saturating_sub(visible_len(prefix));
	Line::from(vec![
		Span::raw("    "),
		Span::styled("at ", Style::default().fg(THEME.nav.meta)),
		Span::styled(
			fit_text(location, value_width, FitMode::Tail),
			Style::default().fg(THEME.nav.meta),
		),
	])
}

fn ref_endpoint_line(endpoint_label: &'static str, endpoint: &str, width: usize) -> Line<'static> {
	let prefix = format!("    {endpoint_label:<6} ");
	let value_width = width.saturating_sub(visible_len(&prefix));
	Line::from(vec![
		Span::raw("    "),
		Span::styled(
			format!("{endpoint_label:<6} "),
			Style::default().fg(THEME.nav.meta),
		),
		Span::raw(fit_text(endpoint, value_width, FitMode::Middle)),
	])
}

fn ref_attrs_line(group: &RefGroup, width: usize) -> Option<Line<'static>> {
	let mut attrs = Vec::new();
	if let Some(receiver) = &group.receiver {
		attrs.push(format!("receiver {receiver}"));
	}
	if let Some(alias) = &group.alias {
		attrs.push(format!("alias {alias}"));
	}
	if attrs.is_empty() {
		return None;
	}
	let prefix = "    via ";
	let value = attrs.join("  ");
	let value_width = width.saturating_sub(visible_len(prefix));
	Some(Line::from(vec![
		Span::raw("    "),
		Span::styled("via ", Style::default().fg(THEME.nav.meta)),
		Span::raw(fit_text(&value, value_width, FitMode::Middle)),
	]))
}

fn ref_location(app: &App, loc: &RefLocation) -> String {
	let file = app.store().file(loc.file);
	let reference = app.store().reference(loc);
	let lines = reference
		.position
		.map(|(start, end)| {
			let (start_line, end_line) = line_range(&file.source, start, end);
			if start_line == end_line {
				format!("L{start_line}")
			} else {
				format!("L{start_line}-L{end_line}")
			}
		})
		.unwrap_or_else(|| "L?".to_string());
	format!("{}:{lines}", file.rel_path.display())
}

fn ref_confidence(reference: &RefRecord) -> String {
	ref_attr(&reference.confidence)
		.map(str::to_string)
		.unwrap_or_else(|| "-".to_string())
}

fn ref_attr(bytes: &[u8]) -> Option<&str> {
	if bytes.is_empty() {
		return None;
	}
	std::str::from_utf8(bytes).ok().filter(|s| !s.is_empty())
}

fn detail_line(label: &str, value: &str, width: usize, mode: FitMode) -> Line<'static> {
	panel::kv(label, value, width, mode)
}

fn display_filter(filter: &str) -> &str {
	if filter.is_empty() { "<all>" } else { filter }
}
