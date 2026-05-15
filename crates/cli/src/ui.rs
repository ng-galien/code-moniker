use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
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
use crate::inspect::{CheckSummary, DefLocation, RefLocation, SessionIndex, SessionOptions};
use crate::{DEFAULT_SCHEME, Exit};
use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::Moniker;

mod filter;
mod navigator;
mod source;
#[cfg(test)]
mod tests;
mod theme;

use filter::{NavFilter, parse_filter};
use navigator::{
	NavNode, NavNodeKind, NavRow, build_navigator, filtered_expanded_keys, flatten_nav,
	is_nav_symbol,
};
use source::source_snippet_lines;
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
	let index = SessionIndex::load(&SessionOptions {
		paths: args.paths.clone(),
		project: args.project.clone(),
		cache_dir: args.cache.clone(),
	})?;
	let app = App::new(index, scheme, args.rules.clone(), args.profile.clone());
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CheckState {
	Pending,
	Ready(CheckSummary),
	Error(String),
}

struct App {
	index: SessionIndex,
	scheme: String,
	rules: PathBuf,
	profile: Option<String>,
	view: View,
	filter: String,
	filter_query: Option<NavFilter>,
	filter_error: Option<String>,
	search_mode: bool,
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
	fn new(index: SessionIndex, scheme: String, rules: PathBuf, profile: Option<String>) -> Self {
		let navigator = build_navigator(&index);
		let mut app = Self {
			index,
			scheme,
			rules,
			profile,
			view: View::Overview,
			filter: String::new(),
			filter_query: None,
			filter_error: None,
			search_mode: false,
			selection: 0,
			visible_defs: Vec::new(),
			navigator,
			expanded: BTreeSet::new(),
			filtered_expanded: BTreeSet::new(),
			nav_rows: Vec::new(),
			check: CheckState::Pending,
			status:
				"Enter opens nodes, / filters tree, kind:<kind> narrows by kind, c checks, q quits"
					.to_string(),
		};
		app.refresh_filter(false);
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
		if self.filter_query.is_some() {
			&self.filtered_expanded
		} else {
			&self.expanded
		}
	}

	fn active_expanded_mut(&mut self) -> &mut BTreeSet<String> {
		if self.filter_query.is_some() {
			&mut self.filtered_expanded
		} else {
			&mut self.expanded
		}
	}

	fn refresh_filter(&mut self, reset_expansion: bool) {
		match parse_filter(&self.filter) {
			Ok(query) => {
				self.filter_error = None;
				self.filter_query = query;
			}
			Err(error) => {
				self.filter_query = None;
				self.filter_error = Some(error.to_string());
				self.visible_defs.clear();
				self.nav_rows.clear();
				self.selection = 0;
				self.status = format!("invalid filter regex: /{}", self.filter);
				return;
			}
		}
		self.visible_defs = self.matching_defs();
		if reset_expansion {
			self.filtered_expanded.clear();
			if let Some(filter) = &self.filter_query {
				let expand_symbols = self.visible_defs.len() <= 200;
				self.filtered_expanded =
					filtered_expanded_keys(&self.index, &self.navigator, filter, expand_symbols);
			}
			self.selection = 0;
		}
		self.refresh_nav();
	}

	fn matching_defs(&self) -> Vec<DefLocation> {
		let mut out: Vec<DefLocation> = self
			.index
			.files
			.iter()
			.enumerate()
			.flat_map(|(file_idx, file)| {
				file.graph
					.defs()
					.enumerate()
					.map(move |(def_idx, _)| DefLocation {
						file: file_idx,
						def: def_idx,
					})
			})
			.filter(|loc| {
				let def = self.index.def(loc);
				if !is_nav_symbol(def) {
					return false;
				}
				self.filter_query
					.as_ref()
					.is_none_or(|filter| filter.matches(&def_kind(def), &last_name(&def.moniker)))
			})
			.collect();
		out.sort_by(|a, b| self.index.def(a).moniker.cmp(&self.index.def(b).moniker));
		out
	}

	fn refresh_nav(&mut self) {
		self.nav_rows.clear();
		if self.filter_error.is_none() {
			let expanded = self.active_expanded().clone();
			let filter = self.filter_query.clone();
			flatten_nav(
				&self.index,
				&self.navigator,
				&expanded,
				filter.as_ref(),
				0,
				&mut self.nav_rows,
			);
		}
		self.clamp_selection();
	}

	fn apply_filter_change(&mut self) {
		self.refresh_filter(true);
		if self.filter_error.is_none() {
			self.status = format!(
				"filter: {} ({}/{})",
				self.filter_label(),
				self.visible_defs.len(),
				self.index.stats.defs
			);
		}
	}

	fn filter_label(&self) -> String {
		self.filter_query
			.as_ref()
			.map(NavFilter::describe)
			.unwrap_or_else(|| display_filter(&self.filter).to_string())
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
			.index
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
		if self.search_mode {
			match key.code {
				KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
					return Ok(true);
				}
				KeyCode::Esc | KeyCode::Enter => {
					self.search_mode = false;
					self.status = format!("filter: {}", self.filter_label());
				}
				KeyCode::Backspace => {
					self.filter.pop();
					self.apply_filter_change();
				}
				KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
					self.filter.push(c);
					self.apply_filter_change();
				}
				_ => {}
			}
			return Ok(false);
		}

		match key.code {
			KeyCode::Char('q') | KeyCode::Esc => Ok(true),
			KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => Ok(true),
			KeyCode::Tab => {
				self.view = self.view.next();
				Ok(false)
			}
			KeyCode::Char('1') => {
				self.view = View::Overview;
				Ok(false)
			}
			KeyCode::Char('2') => {
				self.view = View::Tree;
				Ok(false)
			}
			KeyCode::Char('3') | KeyCode::Char('r') => {
				self.view = View::Refs;
				Ok(false)
			}
			KeyCode::Char('4') => {
				self.view = View::Check;
				Ok(false)
			}
			KeyCode::Char('/') => {
				self.search_mode = true;
				self.status =
					"type a filter: Resolver, kind:interface, kind:method async.*".to_string();
				Ok(false)
			}
			KeyCode::Char('x') => {
				self.filter.clear();
				self.refresh_filter(true);
				self.status = "filter cleared".to_string();
				Ok(false)
			}
			KeyCode::Char('c') => {
				self.run_check();
				Ok(false)
			}
			KeyCode::Down | KeyCode::Char('j') => {
				self.move_down();
				Ok(false)
			}
			KeyCode::Up | KeyCode::Char('k') => {
				self.move_up();
				Ok(false)
			}
			KeyCode::Home | KeyCode::Char('g') => {
				self.selection = 0;
				Ok(false)
			}
			KeyCode::End | KeyCode::Char('G') => {
				self.selection = self.nav_rows.len().saturating_sub(1);
				Ok(false)
			}
			KeyCode::Enter => {
				self.toggle_selected_nav();
				Ok(false)
			}
			KeyCode::Right => {
				self.open_selected_nav();
				Ok(false)
			}
			KeyCode::Left => {
				self.close_selected_nav();
				Ok(false)
			}
			KeyCode::Char('?') => {
				self.status =
					"keys: Enter/right/left tree, / filter, kind:<kind>, x clear, Tab/1-4 views, c check, q quit"
						.to_string();
				Ok(false)
			}
			_ => Ok(false),
		}
	}
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
	let rows = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(1),
			Constraint::Min(0),
			Constraint::Length(1),
		])
		.split(frame.area());
	render_header(frame, rows[0], app);
	render_body(frame, rows[1], app);
	render_footer(frame, rows[2], app);
}

fn render_header(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let stats = &app.index.stats;
	let line = Line::from(vec![
		Span::styled(
			"code-moniker ",
			Style::default()
				.fg(THEME.brand)
				.add_modifier(Modifier::BOLD),
		),
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
	let prefix = if app.search_mode { "search" } else { "status" };
	let line = Line::from(vec![
		Span::styled(
			format!("{prefix}: "),
			Style::default().fg(THEME.status_label),
		),
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
	let title = if app.filter_query.is_some() {
		format!(
			" filtered {} files {} defs ",
			matched_file_count(&app.visible_defs),
			app.visible_defs.len()
		)
	} else if app.filter_error.is_some() {
		" filtered invalid ".to_string()
	} else {
		format!(
			" navigator {} files {} defs ",
			app.index.stats.files, app.navigator.def_count
		)
	};
	let list = List::new(items).block(Block::default().title(title).borders(Borders::ALL));
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
			let def = app.index.def(&loc);
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
	let stats = &app.index.stats;
	let total_ms = stats.scan_ms + stats.extract_ms + stats.index_ms;
	let mut lines = vec![
		Line::raw(format!("root        {}", app.index.root)),
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
	render_panel(frame, area, " overview ", lines);
}

fn render_outline(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let Some(loc) = app.selected() else {
		render_nav_selection(frame, area, app);
		return;
	};
	let file = &app.index.files[loc.file];
	let def = app.index.def(&loc);
	let mut lines = vec![
		Line::styled("selected", Style::default().fg(THEME.section)),
		Line::raw(format!("kind      {}", def_kind(def))),
		Line::raw(format!("name      {}", last_name(&def.moniker))),
		Line::raw(format!("file      {}", file.rel_path.display())),
		Line::raw(format!("moniker   {}", compact_moniker(&def.moniker))),
		Line::raw(""),
		Line::styled("children", Style::default().fg(THEME.section)),
	];
	let children = app
		.index
		.children_by_parent
		.get(&def.moniker)
		.map_or(&[][..], Vec::as_slice);
	if children.is_empty() {
		lines.push(Line::raw("none"));
	} else {
		for child in children.iter().take(40) {
			let child_def = app.index.def(child);
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
	lines.push(Line::styled("source", Style::default().fg(THEME.section)));
	let snippet = source_snippet_lines(app, &loc, 3);
	if snippet.is_empty() {
		lines.push(Line::raw("no source position"));
	} else {
		lines.extend(snippet);
	}
	render_panel_unwrapped(frame, area, " outline ", lines);
}

fn render_nav_selection(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let Some(row) = app.selected_nav_row() else {
		let lines = if let Some(error) = &app.filter_error {
			vec![
				Line::styled("invalid filter", Style::default().fg(THEME.danger)),
				Line::raw(format!("query     {}", app.filter)),
				Line::raw(error),
				Line::raw(""),
				Line::raw("examples  Resolver"),
				Line::raw("          kind:interface Resolver"),
				Line::raw("          kind:method ^async"),
			]
		} else if app.filter_query.is_some() {
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
		render_panel(frame, area, " outline ", lines);
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
	render_panel(frame, area, " outline ", lines);
}

fn render_refs(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let Some(loc) = app.selected() else {
		render_panel(
			frame,
			area,
			" refs ",
			vec![Line::raw("select a declaration to inspect refs")],
		);
		return;
	};
	let def = app.index.def(&loc);
	let outgoing = app.index.outgoing_refs(&def.moniker);
	let incoming = app.index.incoming_refs(&def.moniker);
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
	render_panel(frame, area, " refs ", lines);
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
	render_panel(frame, area, " check ", lines);
}

fn render_panel(frame: &mut ratatui::Frame<'_>, area: Rect, title: &str, lines: Vec<Line<'_>>) {
	let paragraph = Paragraph::new(Text::from(lines))
		.block(Block::default().title(title).borders(Borders::ALL))
		.wrap(Wrap { trim: false });
	frame.render_widget(paragraph, area);
}

fn render_panel_unwrapped(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	title: &str,
	lines: Vec<Line<'_>>,
) {
	let paragraph = Paragraph::new(Text::from(lines))
		.block(Block::default().title(title).borders(Borders::ALL));
	frame.render_widget(paragraph, area);
}

fn ref_line(app: &App, loc: &RefLocation, outgoing: bool) -> Line<'static> {
	let file = &app.index.files[loc.file];
	let reference = app.index.reference(loc);
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

fn display_filter(filter: &str) -> &str {
	if filter.is_empty() { "<all>" } else { filter }
}

fn def_kind(def: &DefRecord) -> String {
	std::str::from_utf8(&def.kind).unwrap_or("?").to_string()
}

fn ref_kind(reference: &RefRecord) -> String {
	std::str::from_utf8(&reference.kind)
		.unwrap_or("?")
		.to_string()
}

fn last_name(moniker: &Moniker) -> String {
	moniker
		.as_view()
		.segments()
		.last()
		.and_then(|s| std::str::from_utf8(s.name).ok())
		.unwrap_or(".")
		.to_string()
}

fn compact_moniker(moniker: &Moniker) -> String {
	let view = moniker.as_view();
	let project = std::str::from_utf8(view.project()).unwrap_or(".");
	let mut out = String::from(project);
	for seg in view.segments() {
		let kind = std::str::from_utf8(seg.kind).unwrap_or("?");
		let name = std::str::from_utf8(seg.name).unwrap_or("?");
		out.push('/');
		out.push_str(kind);
		out.push(':');
		out.push_str(name);
	}
	out
}
