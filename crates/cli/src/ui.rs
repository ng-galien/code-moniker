use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

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
use crate::{DEFAULT_SCHEME, Exit};

mod component;
mod contracts;
mod events;
mod features;
mod filter;
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

	fn label(self) -> &'static str {
		match self {
			Self::Overview => "overview",
			Self::Tree => "outline",
			Self::Refs => "refs",
			Self::Check => "check",
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
				"Enter opens nodes, / edits filter, u focuses usages, c checks, Esc quits ({nav_count} nav items, {command_count} commands)"
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

	fn focus_usages(&mut self, loc: DefLocation) {
		let focus = self.store.usage_focus(loc);
		let label = focus.label.clone();
		self.mode = UiMode::Normal;
		self.filter_draft.clear();
		self.active_filter = ActiveFilter::Usages(focus);
		self.refresh_results(true);
		let focus = self
			.active_filter
			.usage_focus()
			.expect("usage focus was set");
		self.view = View::Refs;
		self.status = format!(
			"usages of {label}: {} reference(s), {} navigable context(s)",
			focus.refs.len(),
			focus.contexts.len()
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
		}
	}

	fn move_up(&mut self) {
		self.selection = self.selection.saturating_sub(1);
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
		self.view = View::Check;
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
			self.view = view;
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
					return Ok(vec![Effect::Navigate(View::Refs.route())]);
				}
			}
			Msg::RunCheck => {
				self.run_check();
				return Ok(vec![Effect::Navigate(View::Check.route())]);
			}
			Msg::MoveDown => self.move_down(),
			Msg::MoveUp => self.move_up(),
			Msg::Home => self.selection = 0,
			Msg::End => self.selection = self.nav_rows.len().saturating_sub(1),
			Msg::ToggleNode => self.toggle_selected_nav(),
			Msg::OpenNode => self.open_selected_nav(),
			Msg::CloseNode => self.close_selected_nav(),
			Msg::Help => {
				self.status =
					"keys: Enter/right/left tree, / edit filter, u usages, x clear, Tab/1-4 views, c check, Esc quit"
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
	let stats = app.store.stats();
	let line = Line::from(vec![
		Span::styled(
			"code-moniker ",
			Style::default()
				.fg(THEME.brand)
				.add_modifier(Modifier::BOLD),
		),
		marker(ComponentId::Header),
		Span::raw(" "),
		Span::raw(format!(
			"{}  files {}  defs {}  refs {}  filter {}",
			app.view.label(),
			stats.files,
			stats.defs,
			stats.refs,
			app.filter_label()
		)),
	]);
	frame.render_widget(Paragraph::new(line), area);
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
			spans.push(Span::styled(
				def_kind(def),
				Style::default().fg(THEME.nav.kind),
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
	if let Some(focus) = app.active_filter.usage_focus() {
		render_usage_focus(frame, area, app, focus);
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
	let outgoing = app.store.outgoing_refs(&def.moniker);
	let incoming = app.store.incoming_refs(&def.moniker);
	let mut lines = vec![
		Line::styled("outgoing", Style::default().fg(THEME.section)),
		Line::raw(format!("{} reference(s)", outgoing.len())),
	];
	for r in outgoing.iter().take(30) {
		lines.push(ref_line(app, r, true));
	}
	if outgoing.len() > 30 {
		lines.push(Line::raw(format!("... {} more", outgoing.len() - 30)));
	}
	lines.push(Line::raw(""));
	lines.push(Line::styled("incoming", Style::default().fg(THEME.section)));
	lines.push(Line::raw(format!("{} reference(s)", incoming.len())));
	for r in incoming.iter().take(30) {
		lines.push(ref_line(app, r, false));
	}
	if incoming.len() > 30 {
		lines.push(Line::raw(format!("... {} more", incoming.len() - 30)));
	}
	render_panel(frame, area, "refs", ComponentId::PanelRefs, lines);
}

fn render_usage_focus(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App, focus: &UsageFocus) {
	let mut lines = vec![
		Line::styled("usage focus", Style::default().fg(THEME.section)),
		Line::raw(format!("symbol     {}", focus.label)),
		Line::raw(format!("moniker    {}", compact_moniker(&focus.target))),
		Line::raw(format!("refs       {}", focus.refs.len())),
		Line::raw(format!("contexts   {}", focus.contexts.len())),
		Line::raw(""),
		Line::styled("references", Style::default().fg(THEME.section)),
	];
	if focus.refs.is_empty() {
		lines.push(Line::raw("none"));
	} else {
		for loc in focus.refs.iter().take(40) {
			lines.push(usage_ref_line(app, loc));
		}
		if focus.refs.len() > 40 {
			lines.push(Line::raw(format!("... {} more", focus.refs.len() - 40)));
		}
	}
	render_panel(frame, area, "usages", ComponentId::PanelUsages, lines);
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

fn ref_line(app: &App, loc: &RefLocation, outgoing: bool) -> Line<'static> {
	let file = app.store.file(loc.file);
	let reference = app.store.reference(loc);
	let source = file.graph.def_at(reference.source);
	let kind = ref_kind(reference);
	let target = compact_moniker(&reference.target);
	let source_name = compact_moniker(&source.moniker);
	let rendered = if outgoing {
		format!("{kind:<14} -> {target}")
	} else {
		format!("{kind:<14} <- {source_name}")
	};
	Line::raw(rendered)
}

fn usage_ref_line(app: &App, loc: &RefLocation) -> Line<'static> {
	let file = app.store.file(loc.file);
	let reference = app.store.reference(loc);
	let source = file.graph.def_at(reference.source);
	Line::raw(format!(
		"{:<14} {}  {}",
		ref_kind(reference),
		file.rel_path.display(),
		compact_moniker(&source.moniker)
	))
}

fn display_filter(filter: &str) -> &str {
	if filter.is_empty() { "<all>" } else { filter }
}
