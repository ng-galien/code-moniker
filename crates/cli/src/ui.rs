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
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use regex::Regex;

use crate::args::UiArgs;
use crate::inspect::{CheckSummary, DefLocation, RefLocation, SessionIndex, SessionOptions};
use crate::{DEFAULT_SCHEME, Exit};
use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::Moniker;

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
		path: args.path.clone(),
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
	search_mode: bool,
	selection: usize,
	visible_defs: Vec<DefLocation>,
	check: CheckState,
	status: String,
}

impl App {
	fn new(index: SessionIndex, scheme: String, rules: PathBuf, profile: Option<String>) -> Self {
		let mut app = Self {
			index,
			scheme,
			rules,
			profile,
			view: View::Overview,
			filter: String::new(),
			search_mode: false,
			selection: 0,
			visible_defs: Vec::new(),
			check: CheckState::Pending,
			status: "Tab changes view, / filters names, c runs check, q quits".to_string(),
		};
		app.refresh_defs();
		app
	}

	fn selected(&self) -> Option<DefLocation> {
		self.visible_defs.get(self.selection).copied()
	}

	fn refresh_defs(&mut self) {
		if !self.filter.is_empty() && Regex::new(&self.filter).is_err() {
			self.visible_defs.clear();
			self.selection = 0;
			self.status = "invalid name regex".to_string();
			return;
		}
		self.visible_defs = self.index.filtered_defs(&crate::inspect::ViewFilter {
			name: (!self.filter.is_empty()).then(|| self.filter.clone()),
			..crate::inspect::ViewFilter::default()
		});
		self.clamp_selection();
	}

	fn clamp_selection(&mut self) {
		if self.visible_defs.is_empty() {
			self.selection = 0;
		} else if self.selection >= self.visible_defs.len() {
			self.selection = self.visible_defs.len() - 1;
		}
	}

	fn move_down(&mut self) {
		if self.selection + 1 < self.visible_defs.len() {
			self.selection += 1;
		}
	}

	fn move_up(&mut self) {
		self.selection = self.selection.saturating_sub(1);
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
				KeyCode::Esc | KeyCode::Enter => {
					self.search_mode = false;
					self.status = format!("filter: {}", display_filter(&self.filter));
				}
				KeyCode::Backspace => {
					self.filter.pop();
					self.refresh_defs();
				}
				KeyCode::Char(c)
					if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
				{
					self.filter.push(c);
					self.refresh_defs();
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
				self.status = "type a Rust regex for declaration names".to_string();
				Ok(false)
			}
			KeyCode::Char('x') => {
				self.filter.clear();
				self.refresh_defs();
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
				self.selection = self.visible_defs.len().saturating_sub(1);
				Ok(false)
			}
			KeyCode::Char('?') => {
				self.status =
					"keys: Tab/1-4 views, arrows/j/k move, / filter, x clear, c check, q quit"
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
				.fg(Color::Cyan)
				.add_modifier(Modifier::BOLD),
		),
		Span::raw(format!(
			"{}  files {}  defs {}  refs {}  filter {}",
			app.view.label(),
			stats.files,
			stats.defs,
			stats.refs,
			display_filter(&app.filter)
		)),
	]);
	frame.render_widget(Paragraph::new(line), area);
}

fn render_body(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let cols = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
		.split(area);
	render_def_list(frame, cols[0], app);
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
		Span::styled(format!("{prefix}: "), Style::default().fg(Color::Yellow)),
		Span::raw(&app.status),
	]);
	frame.render_widget(Paragraph::new(line), area);
}

fn render_def_list(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let visible_rows = area.height.saturating_sub(2) as usize;
	let start = if visible_rows == 0 {
		0
	} else {
		app.selection.saturating_sub(visible_rows.saturating_sub(1))
	};
	let end = (start + visible_rows).min(app.visible_defs.len());
	let items: Vec<ListItem<'_>> = app.visible_defs[start..end]
		.iter()
		.enumerate()
		.map(|(offset, loc)| {
			let idx = start + offset;
			let file = &app.index.files[loc.file];
			let def = app.index.def(loc);
			let marker = if idx == app.selection { ">" } else { " " };
			let line = Line::from(vec![
				Span::styled(marker, Style::default().fg(Color::Yellow)),
				Span::raw(" "),
				Span::styled(def_kind(def), Style::default().fg(Color::Magenta)),
				Span::raw(" "),
				Span::styled(last_name(&def.moniker), Style::default().fg(Color::White)),
				Span::raw("  "),
				Span::styled(
					file.rel_path.display().to_string(),
					Style::default().fg(Color::DarkGray),
				),
			]);
			let style = if idx == app.selection {
				Style::default().bg(Color::DarkGray)
			} else {
				Style::default()
			};
			ListItem::new(line).style(style)
		})
		.collect();
	let title = format!(
		" declarations {}/{} ",
		app.visible_defs.len(),
		app.index.stats.defs
	);
	let list = List::new(items).block(Block::default().title(title).borders(Borders::ALL));
	frame.render_widget(list, area);
}

fn render_overview(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let stats = &app.index.stats;
	let total_ms = stats.scan_ms + stats.extract_ms + stats.index_ms;
	let mut lines = vec![
		Line::raw(format!("root        {}", app.index.root.display())),
		Line::raw(format!("files       {}", stats.files)),
		Line::raw(format!("defs        {}", stats.defs)),
		Line::raw(format!("refs        {}", stats.refs)),
		Line::raw(format!("time        {total_ms} ms")),
		Line::raw(format!("scan        {} ms", stats.scan_ms)),
		Line::raw(format!("extract     {} ms", stats.extract_ms)),
		Line::raw(format!("index       {} ms", stats.index_ms)),
		Line::raw(""),
		Line::styled("languages", Style::default().fg(Color::Cyan)),
	];
	for (lang, totals) in &stats.by_lang {
		lines.push(Line::raw(format!(
			"{lang:<10} files {:>5}  defs {:>7}  refs {:>7}",
			totals.files, totals.defs, totals.refs
		)));
	}
	lines.push(Line::raw(""));
	lines.push(Line::styled("shapes", Style::default().fg(Color::Cyan)));
	for (shape, count) in &stats.by_shape {
		lines.push(Line::raw(format!("{shape:<10} {count}")));
	}
	render_panel(frame, area, " overview ", lines);
}

fn render_outline(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let Some(loc) = app.selected() else {
		render_panel(
			frame,
			area,
			" outline ",
			vec![Line::raw("no declaration matches filter")],
		);
		return;
	};
	let file = &app.index.files[loc.file];
	let def = app.index.def(&loc);
	let mut lines = vec![
		Line::styled("selected", Style::default().fg(Color::Cyan)),
		Line::raw(format!("kind      {}", def_kind(def))),
		Line::raw(format!("name      {}", last_name(&def.moniker))),
		Line::raw(format!("file      {}", file.rel_path.display())),
		Line::raw(format!("moniker   {}", compact_moniker(&def.moniker))),
		Line::raw(""),
		Line::styled("children", Style::default().fg(Color::Cyan)),
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
	lines.push(Line::styled("source", Style::default().fg(Color::Cyan)));
	let snippet = app.index.source_snippet(&loc, 3);
	if snippet.is_empty() {
		lines.push(Line::raw("no source position"));
	} else {
		lines.extend(snippet.into_iter().map(Line::raw));
	}
	render_panel(frame, area, " outline ", lines);
}

fn render_refs(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let Some(loc) = app.selected() else {
		render_panel(
			frame,
			area,
			" refs ",
			vec![Line::raw("no declaration matches filter")],
		);
		return;
	};
	let def = app.index.def(&loc);
	let outgoing = app.index.outgoing_refs(&def.moniker);
	let incoming = app.index.incoming_refs(&def.moniker);
	let mut lines = vec![
		Line::styled("outgoing", Style::default().fg(Color::Cyan)),
		Line::raw(format!("{} reference(s)", outgoing.len())),
	];
	for r in outgoing.iter().take(30) {
		lines.push(ref_line(app, r, true));
	}
	if outgoing.len() > 30 {
		lines.push(Line::raw(format!("... {} more", outgoing.len() - 30)));
	}
	lines.push(Line::raw(""));
	lines.push(Line::styled("incoming", Style::default().fg(Color::Cyan)));
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
			Line::styled("check failed", Style::default().fg(Color::Red)),
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

#[cfg(test)]
mod tests {
	use super::*;

	fn write(root: &std::path::Path, rel: &str, body: &str) {
		let p = root.join(rel);
		if let Some(parent) = p.parent() {
			std::fs::create_dir_all(parent).unwrap();
		}
		std::fs::write(p, body).unwrap();
	}

	#[test]
	fn app_filter_limits_visible_declarations() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"src/a.ts",
			"class Alpha {}\nclass Beta {}\nfunction gamma() {}\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			path: tmp.path().into(),
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		let mut app = App::new(
			index,
			DEFAULT_SCHEME.to_string(),
			tmp.path().join(".code-moniker.toml"),
			None,
		);
		app.filter = "Alpha".into();
		app.refresh_defs();
		assert!(
			app.visible_defs
				.iter()
				.all(|loc| last_name(&app.index.def(loc).moniker).contains("Alpha")),
			"{:?}",
			app.visible_defs
		);
		assert!(!app.visible_defs.is_empty());
	}
}
