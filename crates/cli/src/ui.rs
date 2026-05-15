use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
	EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::args::UiArgs;
use crate::inspect::{CheckSummary, DefLocation, RefLocation, SessionOptions};
use crate::lines::line_range;
use crate::{DEFAULT_SCHEME, Exit};

mod component;
mod contracts;
mod events;
mod features;
mod filter;
mod kinds;
mod navigator;
mod shell;
mod source;
mod store;
#[cfg(test)]
mod tests;
mod theme;

use component::{ComponentId, block_title, marker};
use contracts::{Effect, RenderContext, Route, Screen, ScreenContext};
use events::{FilterEdit, Msg, UiMode, key_to_msg};
use features::explorer::{ExplorerFeature, ROUTE_CHECK, ROUTE_OUTLINE, ROUTE_OVERVIEW, ROUTE_REFS};
use filter::{NavFilter, parse_filter};
use kinds::{definition_kind_group, reference_kind_group, sort_reference_kinds};
use navigator::{
	NavNode, NavNodeKind, NavRow, build_navigator, filtered_expanded_keys, flatten_nav,
};
use shell::FeatureRegistry;
use source::source_snippet_lines;
use store::{
	IndexStore, MemoryIndexStore, UsageFocus, compact_moniker, def_kind, last_name, ref_kind,
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
	let store = MemoryIndexStore::load(&SessionOptions {
		paths: args.paths.clone(),
		project: args.project.clone(),
		cache_dir: args.cache.clone(),
	})?;
	let app = App::new(store, scheme, args.rules.clone(), args.profile.clone());
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
	loop {
		terminal.draw(|frame| draw(frame, app))?;
		if !event::poll(Duration::from_millis(200))? {
			continue;
		}
		let Event::Key(key) = event::read()? else {
			continue;
		};
		if key.kind == KeyEventKind::Press && app.handle_key(key)? {
			return Ok(());
		}
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum View {
	Overview,
	Tree,
	Refs,
	Check,
}

impl View {
	fn next(self) -> Self {
		match self {
			Self::Overview => Self::Tree,
			Self::Tree => Self::Refs,
			Self::Refs => Self::Check,
			Self::Check => Self::Overview,
		}
	}

	fn route_path(self) -> &'static str {
		match self {
			Self::Overview => ROUTE_OVERVIEW,
			Self::Tree => ROUTE_OUTLINE,
			Self::Refs => ROUTE_REFS,
			Self::Check => ROUTE_CHECK,
		}
	}

	fn from_route_path(path: &str) -> Option<Self> {
		match path {
			ROUTE_OVERVIEW => Some(Self::Overview),
			ROUTE_OUTLINE => Some(Self::Tree),
			ROUTE_REFS => Some(Self::Refs),
			ROUTE_CHECK => Some(Self::Check),
			_ => None,
		}
	}

	fn route(self) -> Route {
		ExplorerFeature::route(self.route_path())
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum VisualizationRegime {
	Explorer,
	Search,
	Usages,
}

impl VisualizationRegime {
	fn label(self) -> &'static str {
		match self {
			Self::Explorer => "explorer",
			Self::Search => "search",
			Self::Usages => "usages",
		}
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PanelPolicy {
	Contextual,
	Manual,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CheckState {
	Pending,
	Ready(CheckSummary),
	Error(String),
}

#[derive(Clone, Debug)]
enum ActiveFilter {
	None,
	Text { raw: String, query: NavFilter },
	Invalid { raw: String, error: String },
	Usages(UsageFocus),
}

impl ActiveFilter {
	fn label(&self) -> String {
		match self {
			Self::None => "<all>".to_string(),
			Self::Text { query, .. } => query.describe(),
			Self::Invalid { raw, .. } => display_filter(raw).to_string(),
			Self::Usages(focus) => format!("usages:{}", focus.label),
		}
	}

	fn text_raw(&self) -> Option<&str> {
		match self {
			Self::Text { raw, .. } | Self::Invalid { raw, .. } => Some(raw),
			Self::None | Self::Usages(_) => None,
		}
	}

	fn query(&self) -> Option<&NavFilter> {
		match self {
			Self::Text { query, .. } => Some(query),
			Self::None | Self::Invalid { .. } | Self::Usages(_) => None,
		}
	}

	fn usage_focus(&self) -> Option<&UsageFocus> {
		match self {
			Self::Usages(focus) => Some(focus),
			Self::None | Self::Text { .. } | Self::Invalid { .. } => None,
		}
	}

	fn error(&self) -> Option<(&str, &str)> {
		match self {
			Self::Invalid { raw, error } => Some((raw, error)),
			Self::None | Self::Text { .. } | Self::Usages(_) => None,
		}
	}

	fn filters_navigator(&self) -> bool {
		matches!(self, Self::Text { .. } | Self::Usages(_))
	}
}

struct App {
	registry: FeatureRegistry,
	route: Route,
	store: MemoryIndexStore,
	scheme: String,
	rules: PathBuf,
	profile: Option<String>,
	view: View,
	regime: VisualizationRegime,
	panel_policy: PanelPolicy,
	mode: UiMode,
	active_filter: ActiveFilter,
	filter_draft: String,
	selection: usize,
	visible_defs: Vec<DefLocation>,
	navigator: NavNode,
	expanded: BTreeSet<String>,
	filtered_expanded: BTreeSet<String>,
	nav_rows: Vec<NavRow>,
	check: CheckState,
	status: String,
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
		let mut app = Self {
			registry,
			route,
			store,
			scheme,
			rules,
			profile,
			view: View::Overview,
			regime: VisualizationRegime::Explorer,
			panel_policy: PanelPolicy::Contextual,
			mode: UiMode::Normal,
			active_filter: ActiveFilter::None,
			filter_draft: String::new(),
			selection: 0,
			visible_defs: Vec::new(),
			navigator,
			expanded: BTreeSet::new(),
			filtered_expanded: BTreeSet::new(),
			nav_rows: Vec::new(),
			check: CheckState::Pending,
			status: format!(
				"Enter opens nodes, Esc/left closes, / edits filter, u focuses usages, c checks, q quits ({nav_count} nav items, {command_count} commands)"
			),
		};
		app.refresh_results(false);
		app
	}

	fn selected(&self) -> Option<DefLocation> {
		self.selected_nav_row().and_then(|row| match row.kind {
			NavNodeKind::Def(loc) => Some(loc),
			_ => None,
		})
	}

	fn selected_nav_row(&self) -> Option<&NavRow> {
		self.nav_rows.get(self.selection)
	}

	fn active_expanded(&self) -> &BTreeSet<String> {
		if self.is_filtered() {
			&self.filtered_expanded
		} else {
			&self.expanded
		}
	}

	fn active_expanded_mut(&mut self) -> &mut BTreeSet<String> {
		if self.is_filtered() {
			&mut self.filtered_expanded
		} else {
			&mut self.expanded
		}
	}

	fn refresh_results(&mut self, reset_expansion: bool) {
		self.visible_defs = self.matching_defs();
		if reset_expansion {
			self.filtered_expanded.clear();
			if self.is_filtered() {
				let expand_symbols = self.visible_defs.len() <= 200;
				self.filtered_expanded =
					filtered_expanded_keys(&self.navigator, &self.visible_defs, expand_symbols);
			}
			self.selection = 0;
		}
		self.refresh_nav();
	}

	fn matching_defs(&self) -> Vec<DefLocation> {
		match &self.active_filter {
			ActiveFilter::Usages(focus) => focus.contexts.clone(),
			ActiveFilter::Invalid { .. } => Vec::new(),
			ActiveFilter::None | ActiveFilter::Text { .. } => {
				self.store.all_navigable_defs(self.active_filter.query())
			}
		}
	}

	fn refresh_nav(&mut self) {
		self.nav_rows.clear();
		if self.active_filter.error().is_none() {
			let expanded = self.active_expanded().clone();
			let matches = self.is_filtered().then_some(self.visible_defs.as_slice());
			flatten_nav(&self.navigator, &expanded, matches, 0, &mut self.nav_rows);
		}
		self.clamp_selection();
	}

	fn filter_label(&self) -> String {
		if self.mode == UiMode::EditingFilter {
			return display_filter(&self.filter_draft).to_string();
		}
		self.active_filter.label()
	}

	fn is_filtered(&self) -> bool {
		self.active_filter.filters_navigator()
	}

	fn contextual_view(&self) -> View {
		match self.regime {
			VisualizationRegime::Usages => View::Refs,
			VisualizationRegime::Search if self.active_filter.error().is_some() => View::Tree,
			VisualizationRegime::Explorer | VisualizationRegime::Search => {
				if self.selected().is_some() {
					View::Tree
				} else {
					View::Overview
				}
			}
		}
	}

	fn sync_contextual_view(&mut self) {
		if self.panel_policy == PanelPolicy::Contextual {
			self.set_view(self.contextual_view(), PanelPolicy::Contextual);
		}
	}

	fn set_view(&mut self, view: View, policy: PanelPolicy) {
		self.view = view;
		self.panel_policy = policy;
		self.route = view.route();
	}

	fn scope_label(&self) -> String {
		match &self.active_filter {
			ActiveFilter::None => "all".to_string(),
			ActiveFilter::Text { query, .. } => query.describe(),
			ActiveFilter::Invalid { raw, .. } => format!("invalid {}", display_filter(raw)),
			ActiveFilter::Usages(focus) => focus.label.clone(),
		}
	}

	fn focus_usages(&mut self, loc: DefLocation) {
		let focus = self.store.usage_focus(loc);
		let label = focus.label.clone();
		self.mode = UiMode::Normal;
		self.filter_draft.clear();
		self.regime = VisualizationRegime::Usages;
		self.panel_policy = PanelPolicy::Contextual;
		self.active_filter = ActiveFilter::Usages(focus);
		self.refresh_results(true);
		let (refs_len, contexts_len) = {
			let focus = self
				.active_filter
				.usage_focus()
				.expect("usage focus was set");
			(focus.refs.len(), focus.contexts.len())
		};
		self.sync_contextual_view();
		self.status = format!(
			"usages of {label}: {} reference(s), {} navigable context(s)",
			refs_len, contexts_len
		);
	}

	fn start_filter_edit(&mut self) {
		self.mode = UiMode::EditingFilter;
		self.filter_draft = self
			.active_filter
			.text_raw()
			.map(str::to_string)
			.unwrap_or_default();
		self.status =
			"type a filter, Enter applies, Esc cancels: Resolver, kind:interface, kind:method async.*"
				.to_string();
	}

	fn edit_filter(&mut self, edit: FilterEdit) {
		match edit {
			FilterEdit::Push(c) => self.filter_draft.push(c),
			FilterEdit::Backspace => {
				self.filter_draft.pop();
			}
			FilterEdit::Clear => self.filter_draft.clear(),
		}
		self.status = format!("draft filter: {}", display_filter(&self.filter_draft));
	}

	fn apply_filter(&mut self) {
		let raw = self.filter_draft.trim().to_string();
		self.mode = UiMode::Normal;
		self.active_filter = match parse_filter(&raw) {
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
		self.refresh_results(true);
		self.regime = match &self.active_filter {
			ActiveFilter::None => VisualizationRegime::Explorer,
			ActiveFilter::Text { .. } | ActiveFilter::Invalid { .. } => VisualizationRegime::Search,
			ActiveFilter::Usages(_) => VisualizationRegime::Usages,
		};
		self.panel_policy = PanelPolicy::Contextual;
		self.sync_contextual_view();
		if let Some((raw, _)) = self.active_filter.error() {
			self.status = format!("invalid filter regex: /{raw}");
		} else {
			self.status = format!(
				"filter: {} ({}/{})",
				self.filter_label(),
				self.visible_defs.len(),
				self.store.stats().defs
			);
		}
	}

	fn cancel_input(&mut self) {
		self.mode = UiMode::Normal;
		self.status = format!(
			"filter edit canceled; active filter: {}",
			self.filter_label()
		);
	}

	fn clear_filter(&mut self) {
		self.mode = UiMode::Normal;
		self.regime = VisualizationRegime::Explorer;
		self.panel_policy = PanelPolicy::Contextual;
		self.active_filter = ActiveFilter::None;
		self.filter_draft.clear();
		self.refresh_results(true);
		self.status = "filter cleared".to_string();
	}

	fn focus_usages_of_selected(&mut self) {
		let Some(loc) = self.selected() else {
			self.status = "select a declaration before focusing usages".to_string();
			return;
		};
		self.focus_usages(loc);
	}

	fn clamp_selection(&mut self) {
		let len = self.nav_rows.len();
		if len == 0 {
			self.selection = 0;
		} else if self.selection >= len {
			self.selection = len - 1;
		}
	}

	fn move_down(&mut self) {
		let len = self.nav_rows.len();
		if self.selection + 1 < len {
			self.selection += 1;
			self.sync_contextual_view();
		}
	}

	fn move_up(&mut self) {
		self.selection = self.selection.saturating_sub(1);
		self.sync_contextual_view();
	}

	fn toggle_selected_nav(&mut self) {
		let Some(row) = self.selected_nav_row() else {
			return;
		};
		if !row.has_children {
			return;
		}
		let key = row.key.clone();
		let label = row.label.clone();
		if self.active_expanded_mut().remove(&key) {
			self.status = format!("closed {label}");
		} else {
			self.active_expanded_mut().insert(key);
			self.status = format!("opened {label}");
		}
		self.refresh_nav();
	}

	fn open_selected_nav(&mut self) {
		let Some(row) = self.selected_nav_row() else {
			return;
		};
		if row.has_children && !self.active_expanded().contains(&row.key) {
			let key = row.key.clone();
			let label = row.label.clone();
			self.active_expanded_mut().insert(key);
			self.status = format!("opened {label}");
			self.refresh_nav();
		}
	}

	fn close_selected_nav(&mut self) {
		let Some(row) = self.selected_nav_row() else {
			return;
		};
		if row.has_children && self.active_expanded().contains(&row.key) {
			let key = row.key.clone();
			let label = row.label.clone();
			self.active_expanded_mut().remove(&key);
			self.status = format!("closed {label}");
			self.refresh_nav();
			return;
		}
		if row.depth == 0 {
			return;
		}
		let parent_depth = row.depth - 1;
		if let Some(parent) = self.nav_rows[..self.selection]
			.iter()
			.rposition(|candidate| candidate.depth == parent_depth)
		{
			self.selection = parent;
		}
	}

	fn run_check(&mut self) {
		self.set_view(View::Check, PanelPolicy::Manual);
		match self
			.store
			.check_summary(&self.rules, self.profile.as_deref(), &self.scheme)
		{
			Ok(summary) => {
				self.status = format!(
					"check complete: {} violation(s) across {} file(s)",
					summary.total_violations, summary.files_with_violations
				);
				self.check = CheckState::Ready(summary);
			}
			Err(e) => {
				self.status = "check failed".to_string();
				self.check = CheckState::Error(e.to_string());
			}
		}
	}

	fn handle_key(&mut self, key: KeyEvent) -> anyhow::Result<bool> {
		Ok(self.update(Msg::Key(key)))
	}

	fn update(&mut self, msg: Msg) -> bool {
		let msg = match msg {
			Msg::Key(key) => key_to_msg(self.mode, key),
			msg => msg,
		};
		let route = self.route.clone();
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
			Effect::Notify(message) => self.status = message,
			Effect::Refresh => self.refresh_results(false),
			Effect::Spawn(task) => {
				self.status = format!("task queued: {} ({})", task.label, task.id);
			}
			Effect::None => {}
		}
		false
	}

	fn navigate(&mut self, route: Route) {
		if !self.registry.can_open(&route) {
			self.status = format!("unknown route: {}/{}", route.feature, route.path);
			return;
		}
		if let Some(view) = View::from_route_path(&route.path) {
			self.set_view(view, PanelPolicy::Manual);
			return;
		}
		self.route = route;
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
			Msg::CycleView => return Ok(vec![Effect::Navigate(self.view.next().route())]),
			Msg::ShowView(view) => return Ok(vec![Effect::Navigate(view.route())]),
			Msg::StartFilterEdit => self.start_filter_edit(),
			Msg::FilterInput(edit) => self.edit_filter(edit),
			Msg::ApplyFilter => self.apply_filter(),
			Msg::CancelInput => self.cancel_input(),
			Msg::ClearFilter => self.clear_filter(),
			Msg::FocusUsages => {
				let had_selection = self.selected().is_some();
				self.focus_usages_of_selected();
				if had_selection {
					return Ok(Vec::new());
				}
			}
			Msg::RunCheck => {
				self.run_check();
				return Ok(Vec::new());
			}
			Msg::MoveDown => self.move_down(),
			Msg::MoveUp => self.move_up(),
			Msg::Home => {
				self.selection = 0;
				self.sync_contextual_view();
			}
			Msg::End => {
				self.selection = self.nav_rows.len().saturating_sub(1);
				self.sync_contextual_view();
			}
			Msg::ToggleNode => self.toggle_selected_nav(),
			Msg::OpenNode => self.open_selected_nav(),
			Msg::CloseNode => self.close_selected_nav(),
			Msg::Help => {
				self.status =
					"keys: Enter/right open, Esc/left close, / filter, u usages, x clear, Tab/1-4 panels, c check, q quit"
						.to_string();
			}
			Msg::Key(_) | Msg::Noop => {}
		}
		Ok(Vec::new())
	}
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &mut App) {
	let _title = Screen::title(app);
	let _component = Screen::component(app);
	let route = app.route.clone();
	let ctx = RenderContext { route: &route };
	Screen::render(app, frame, frame.area(), &ctx);
}

fn render_shell(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
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
	let regime = app.regime.label();
	let prefix_width = visible_len("code-moniker ")
		+ visible_len(ComponentId::Header.as_str())
		+ 2 + visible_len(" regime ")
		+ visible_len(regime)
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
		Span::raw("regime "),
		Span::styled(
			app.regime.label(),
			Style::default()
				.fg(THEME.section)
				.add_modifier(Modifier::BOLD),
		),
		Span::raw("  scope "),
		Span::styled(scope, Style::default().fg(THEME.nav.symbol)),
	])
}

fn render_body(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let cols = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
		.split(area);
	render_nav_list(frame, cols[0], app);
	match app.view {
		View::Overview => render_overview(frame, cols[1], app),
		View::Tree => render_outline(frame, cols[1], app),
		View::Refs => render_refs(frame, cols[1], app),
		View::Check => render_check(frame, cols[1], app),
	}
}

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let prefix = if app.mode == UiMode::EditingFilter {
		"filter"
	} else {
		"status"
	};
	let line = Line::from(vec![
		Span::styled(
			format!("{prefix}: "),
			Style::default().fg(THEME.status_label),
		),
		marker(ComponentId::Status),
		Span::raw(" "),
		Span::raw(&app.status),
	]);
	frame.render_widget(Paragraph::new(line), area);
}

fn render_nav_list(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let visible_rows = area.height.saturating_sub(2) as usize;
	let start = if visible_rows == 0 {
		0
	} else {
		app.selection.saturating_sub(visible_rows.saturating_sub(1))
	};
	let end = (start + visible_rows).min(app.nav_rows.len());
	let items: Vec<ListItem<'_>> = app.nav_rows[start..end]
		.iter()
		.enumerate()
		.map(|(offset, row)| {
			let idx = start + offset;
			let line = nav_row_line(app, row, idx == app.selection);
			let style = if idx == app.selection {
				Style::default().bg(THEME.nav.selected_bg)
			} else {
				Style::default()
			};
			ListItem::new(line).style(style)
		})
		.collect();
	let title = if app.is_filtered() {
		format!(
			" filtered {} files {} defs ",
			matched_file_count(&app.visible_defs),
			app.visible_defs.len()
		)
	} else if app.active_filter.error().is_some() {
		" filtered invalid ".to_string()
	} else {
		format!(
			" navigator {} files {} defs ",
			app.store.stats().files,
			app.navigator.def_count
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
			spans.push(Span::styled(
				row.label.clone(),
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
		NavNodeKind::File(_) => {
			spans.push(Span::styled(
				row.label.clone(),
				Style::default()
					.fg(THEME.nav.file)
					.add_modifier(Modifier::BOLD),
			));
			spans.push(nav_count_span(row));
		}
		NavNodeKind::Def(loc) => {
			let def = app.store.def(&loc);
			let kind = def_kind(def);
			let group = definition_kind_group(app.store.file(loc.file).lang, &kind);
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

fn matched_file_count(defs: &[DefLocation]) -> usize {
	defs.iter()
		.map(|loc| loc.file)
		.collect::<BTreeSet<_>>()
		.len()
}

fn render_overview(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let stats = app.store.stats();
	let total_ms = stats.scan_ms + stats.extract_ms + stats.index_ms;
	let mut lines = vec![
		Line::raw(format!("root        {}", app.store.root())),
		Line::raw(format!("files       {}", stats.files)),
		Line::raw(format!("defs        {}", stats.defs)),
		Line::raw(format!("refs        {}", stats.refs)),
		Line::raw(format!("time        {total_ms} ms")),
		Line::raw(format!("scan        {} ms", stats.scan_ms)),
		Line::raw(format!("extract     {} ms", stats.extract_ms)),
		Line::raw(format!("index       {} ms", stats.index_ms)),
		Line::raw(""),
		Line::styled("languages", Style::default().fg(THEME.section)),
	];
	for (lang, totals) in &stats.by_lang {
		lines.push(Line::raw(format!(
			"{lang:<10} files {:>5}  defs {:>7}  refs {:>7}",
			totals.files, totals.defs, totals.refs
		)));
	}
	lines.push(Line::raw(""));
	lines.push(Line::styled("shapes", Style::default().fg(THEME.section)));
	for (shape, count) in &stats.by_shape {
		lines.push(Line::raw(format!("{shape:<10} {count}")));
	}
	render_panel(frame, area, "overview", ComponentId::PanelOverview, lines);
}

fn render_outline(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let Some(loc) = app.selected() else {
		render_nav_selection(frame, area, app);
		return;
	};
	let file = app.store.file(loc.file);
	let def = app.store.def(&loc);
	let mut lines = vec![
		Line::styled("selected", Style::default().fg(THEME.section)),
		Line::raw(format!("kind      {}", def_kind(def))),
		Line::raw(format!("name      {}", last_name(&def.moniker))),
		Line::raw(format!("file      {}", file.rel_path.display())),
		Line::raw(format!("moniker   {}", compact_moniker(&def.moniker))),
		Line::raw(""),
		Line::styled("children", Style::default().fg(THEME.section)),
	];
	let children = app.store.children_by_parent(&def.moniker);
	if children.is_empty() {
		lines.push(Line::raw("none"));
	} else {
		for child in children.iter().take(40) {
			let child_def = app.store.def(child);
			lines.push(Line::raw(format!(
				"{} {}",
				def_kind(child_def),
				last_name(&child_def.moniker)
			)));
		}
		if children.len() > 40 {
			lines.push(Line::raw(format!("... {} more", children.len() - 40)));
		}
	}
	lines.push(Line::raw(""));
	lines.push(Line::from(vec![
		Span::styled("source", Style::default().fg(THEME.section)),
		Span::raw(" "),
		marker(ComponentId::SourceSnippet),
	]));
	let snippet = source_snippet_lines(app, &loc, 3);
	if snippet.is_empty() {
		lines.push(Line::raw("no source position"));
	} else {
		lines.extend(snippet);
	}
	render_panel_unwrapped(frame, area, "outline", ComponentId::PanelOutline, lines);
}

fn render_nav_selection(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let Some(row) = app.selected_nav_row() else {
		let lines = if let Some((raw, error)) = app.active_filter.error() {
			vec![
				Line::styled("invalid filter", Style::default().fg(THEME.danger)),
				Line::raw(format!("query     {raw}")),
				Line::raw(error),
				Line::raw(""),
				Line::raw("examples  Resolver"),
				Line::raw("          kind:interface Resolver"),
				Line::raw("          kind:method ^async"),
			]
		} else if app.is_filtered() {
			vec![
				Line::styled("filtered navigator", Style::default().fg(THEME.section)),
				Line::raw(format!("filter    {}", app.filter_label())),
				Line::raw("matches   0"),
				Line::raw(""),
				Line::raw("x clears the filter"),
			]
		} else {
			vec![Line::raw("navigator is empty")]
		};
		render_panel(frame, area, "outline", ComponentId::PanelOutline, lines);
		return;
	};
	let kind = match row.kind {
		NavNodeKind::Root => "root",
		NavNodeKind::Lang => "language",
		NavNodeKind::Dir => "directory",
		NavNodeKind::File(_) => "file",
		NavNodeKind::Def(_) => "declaration",
	};
	let mut lines = vec![
		Line::styled("navigator", Style::default().fg(THEME.section)),
		Line::raw(format!("kind      {kind}")),
		Line::raw(format!("name      {}", row.label)),
		Line::raw(format!("files     {}", row.file_count)),
		Line::raw(format!("defs      {}", row.def_count)),
		Line::raw(""),
	];
	if row.has_children {
		let state = if app.active_expanded().contains(&row.key) {
			"opened"
		} else {
			"closed"
		};
		lines.push(Line::raw(format!("state     {state}")));
		lines.push(Line::raw("Enter toggles, right opens, left closes"));
	} else {
		lines.push(Line::raw("no child node"));
	}
	render_panel(frame, area, "outline", ComponentId::PanelOutline, lines);
}

fn render_refs(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let width = panel_content_width(area);
	if let Some(focus) = app.active_filter.usage_focus() {
		render_usage_focus(frame, area, app, focus, width);
		return;
	}
	let Some(loc) = app.selected() else {
		render_panel(
			frame,
			area,
			"refs",
			ComponentId::PanelRefs,
			vec![Line::raw("select a declaration to inspect refs")],
		);
		return;
	};
	let def = app.store.def(&loc);
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

fn refs_panel_lines(
	app: &App,
	loc: DefLocation,
	def: &DefRecord,
	width: usize,
) -> Vec<Line<'static>> {
	let file = app.store.file(loc.file);
	let outgoing = app.store.outgoing_refs(&def.moniker);
	let incoming = app.store.incoming_refs(&def.moniker);
	let mut lines = vec![
		Line::styled("selected", Style::default().fg(THEME.section)),
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
		Line::raw(""),
		Line::styled("incoming impact", Style::default().fg(THEME.section)),
		Line::raw(reference_summary(app, incoming)),
	];
	push_ref_rows(&mut lines, app, incoming, RefDirection::Incoming, 30, width);
	lines.push(Line::raw(""));
	lines.push(Line::styled(
		"outgoing dependencies",
		Style::default().fg(THEME.section),
	));
	lines.push(Line::raw(reference_summary(app, outgoing)));
	push_ref_rows(&mut lines, app, outgoing, RefDirection::Outgoing, 30, width);
	lines
}

fn usage_focus_lines(app: &App, focus: &UsageFocus, width: usize) -> Vec<Line<'static>> {
	let mut lines = vec![
		Line::styled("usage focus", Style::default().fg(THEME.section)),
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
		Line::raw(""),
		Line::styled("references", Style::default().fg(THEME.section)),
	];
	if focus.refs.is_empty() {
		lines.push(Line::raw("none"));
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

fn render_check(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let lines = match &app.check {
		CheckState::Pending => vec![
			Line::raw("press c to run .code-moniker.toml rules on the loaded graph"),
			Line::raw(format!("rules   {}", app.rules.display())),
			Line::raw(format!(
				"profile {}",
				app.profile.as_deref().unwrap_or("<none>")
			)),
		],
		CheckState::Ready(summary) => vec![
			Line::raw(format!("files scanned          {}", summary.files_scanned)),
			Line::raw(format!(
				"files with violations  {}",
				summary.files_with_violations
			)),
			Line::raw(format!(
				"violations             {}",
				summary.total_violations
			)),
		],
		CheckState::Error(error) => vec![
			Line::styled("check failed", Style::default().fg(THEME.danger)),
			Line::raw(error),
		],
	};
	render_panel(frame, area, "check", ComponentId::PanelCheck, lines);
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum FitMode {
	Middle,
	Tail,
}

fn reference_summary(app: &App, refs: &[RefLocation]) -> String {
	let files = refs
		.iter()
		.map(|loc| app.store.file(loc.file).rel_path.as_path())
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
		lines.push(Line::raw("none"));
		return;
	}
	let groups = ref_groups(app, refs, direction);
	for group in groups.iter().take(limit) {
		lines.extend(ref_group_lines(group, width));
	}
	if groups.len() > limit {
		lines.push(Line::raw(format!("... {} more", groups.len() - limit)));
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
	let file = app.store.file(loc.file);
	let reference = app.store.reference(loc);
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
	let file = app.store.file(loc.file);
	let reference = app.store.reference(loc);
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
	let prefix = format!("{label:<10}");
	let value_width = width.saturating_sub(visible_len(&prefix));
	Line::raw(format!("{prefix}{}", fit_text(value, value_width, mode)))
}

fn fit_text(value: &str, width: usize, mode: FitMode) -> String {
	if visible_len(value) <= width {
		return value.to_string();
	}
	match mode {
		FitMode::Middle => fit_middle(value, width),
		FitMode::Tail => fit_tail(value, width),
	}
}

fn fit_middle(value: &str, width: usize) -> String {
	if width == 0 {
		return String::new();
	}
	if width <= 3 {
		return ".".repeat(width);
	}
	let available = width - 3;
	let left = available / 2;
	let right = available - left;
	format!("{}...{}", take_start(value, left), take_end(value, right))
}

fn fit_tail(value: &str, width: usize) -> String {
	if width == 0 {
		return String::new();
	}
	if width <= 3 {
		return ".".repeat(width);
	}
	format!("...{}", take_end(value, width - 3))
}

fn take_start(value: &str, count: usize) -> String {
	value.chars().take(count).collect()
}

fn take_end(value: &str, count: usize) -> String {
	let chars: Vec<_> = value.chars().collect();
	chars[chars.len().saturating_sub(count)..].iter().collect()
}

fn visible_len(value: &str) -> usize {
	value.chars().count()
}

fn display_filter(filter: &str) -> &str {
	if filter.is_empty() { "<all>" } else { filter }
}
