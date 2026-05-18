use std::collections::BTreeSet;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use crate::workspace::{ChangeStatus, DefLocation, IndexStore};

use super::app::VisualizationMode;
use super::component::{ComponentId, focused_block_title, marker, raw_marker};
use super::contracts::{RenderContext, Screen};
use super::events::{HeaderSearchFocus, UiMode};
use super::features::explorer::ExplorerFeature;
use super::kinds::definition_kind_group;
use super::navigator::{NavNodeKind, NavRow};
use super::panel;
use super::panels::{self, PanelRenderState};
use super::scroll::{ScrollViewport, render_vertical_scrollbar, viewport_comfort_margin};
use super::text::{FitMode, fit_text, visible_len};
use super::theme::THEME;
use super::{
	App, DEFAULT_PANEL_SNAPSHOT_WIDTH, FocusRegion, display_filter, kind_filter_summary,
	lang_filter_summary,
};

const SEARCH_WIDGET_HEIGHT: u16 = 5;

pub(super) fn draw(frame: &mut ratatui::Frame<'_>, app: &mut App) {
	let route = app.route().clone();
	let ctx = RenderContext { route: &route };
	let mut screen = ExplorerFeature::screen(app);
	let _title = screen.title();
	let _component = screen.component();
	screen.render(frame, frame.area(), &ctx);
}

pub(in crate::ui) fn render_shell(frame: &mut ratatui::Frame<'_>, area: Rect, app: &mut App) {
	let rows = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(1),
			Constraint::Length(SEARCH_WIDGET_HEIGHT),
			Constraint::Min(0),
			Constraint::Length(1),
		])
		.split(area);
	render_header(frame, rows[0], app);
	render_search_bar(frame, rows[1], app);
	render_body(frame, rows[2], app);
	render_footer(frame, rows[3], app);
	render_search_popup(frame, rows[1], app);
}

fn render_header(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	frame.render_widget(
		Paragraph::new(header_line(app, usize::from(area.width))),
		area,
	);
}

pub(super) fn header_line(app: &App, width: usize) -> Line<'static> {
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

fn render_search_bar(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let focused = matches!(app.mode(), UiMode::HeaderSearch(_));
	let block = Block::default()
		.title(focused_block_title(
			" search ",
			ComponentId::SearchInput,
			focused,
		))
		.borders(Borders::ALL)
		.border_style(if focused {
			Style::default().fg(THEME.focus.border)
		} else {
			Style::default()
		})
		.style(Style::default().bg(THEME.search.background));
	let inner = block.inner(area);
	frame.render_widget(block, area);
	let regions = search_regions(app, inner);
	render_search_query(frame, regions.query, app);
	render_search_combo(
		frame,
		regions.lang,
		"lang",
		search_lang_summary(app),
		app,
		HeaderSearchFocus::Lang,
	);
	render_search_combo(
		frame,
		regions.kind,
		"kind",
		search_kind_summary(app),
		app,
		HeaderSearchFocus::Kind,
	);
}

#[cfg(test)]
pub(super) fn search_line(app: &App, width: usize) -> Line<'static> {
	let search = app.header_search();
	let raw_search_value = display_filter(search.text.trim()).to_string();
	let raw_lang_value = search_lang_summary(app);
	let raw_kind_value = search_kind_summary(app);
	let fixed_width = visible_len("query [] filters lang [] kind []");
	let [search_width, lang_width, kind_width] = fit_search_value_widths(
		width.saturating_sub(fixed_width),
		[
			visible_len(&raw_search_value),
			visible_len(&raw_lang_value),
			visible_len(&raw_kind_value),
		],
	);
	let search_value = fit_text(&raw_search_value, search_width, FitMode::Middle);
	let lang_value = fit_text(&raw_lang_value, lang_width, FitMode::Middle);
	let kind_value = fit_text(&raw_kind_value, kind_width, FitMode::Middle);
	Line::from(vec![
		Span::raw("query ["),
		Span::raw(search_value),
		Span::raw("] filters "),
		Span::raw("lang ["),
		Span::raw(lang_value),
		Span::raw("] "),
		Span::raw("kind ["),
		Span::raw(kind_value),
		Span::raw("]"),
	])
}

#[derive(Copy, Clone, Debug)]
struct SearchRegions {
	query: Rect,
	lang: Rect,
	kind: Rect,
}

fn search_regions(app: &App, area: Rect) -> SearchRegions {
	let lang_width = combo_width("lang", &search_lang_summary(app), 16, 30);
	let kind_width = combo_width("kind", &search_kind_summary(app), 18, 38);
	let max_selector_width = area.width.saturating_sub(18);
	let selector_width = (lang_width + kind_width).min(max_selector_width);
	let lang_width = lang_width.min(selector_width / 2).max(10);
	let kind_width = selector_width.saturating_sub(lang_width).max(10);
	let chunks = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([
			Constraint::Min(12),
			Constraint::Length(lang_width),
			Constraint::Length(kind_width),
		])
		.split(area);
	SearchRegions {
		query: chunks[0],
		lang: chunks[1],
		kind: chunks[2],
	}
}

fn combo_width(label: &str, value: &str, min: u16, max: u16) -> u16 {
	let requested = visible_len(label) + visible_len(value) + 8;
	requested.clamp(usize::from(min), usize::from(max)) as u16
}

fn render_search_query(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let focused = matches!(app.mode(), UiMode::HeaderSearch(HeaderSearchFocus::Text));
	let block = search_query_block(focused);
	let inner = block.inner(area);
	frame.render_widget(block, area);
	frame.render_widget(Paragraph::new(search_query_line(app, inner.width)), inner);
}

fn render_search_combo(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	label: &'static str,
	value: String,
	app: &App,
	focus: HeaderSearchFocus,
) {
	let focused = matches!(app.mode(), UiMode::HeaderSearch(active) if active == focus);
	let block = search_block(label, focused);
	let inner = block.inner(area);
	frame.render_widget(block, area);
	let fitted = fit_text(
		&format!("{value} ▾"),
		usize::from(inner.width),
		FitMode::Middle,
	);
	frame.render_widget(
		Paragraph::new(Line::from(Span::styled(
			fitted,
			search_value_style(focused),
		))),
		inner,
	);
}

fn search_block(label: &'static str, focused: bool) -> Block<'static> {
	search_block_with_title(
		Line::from(Span::styled(
			format!(" {label} "),
			Style::default().fg(THEME.search.label),
		)),
		focused,
	)
}

fn search_query_block(focused: bool) -> Block<'static> {
	search_block_with_title(
		Line::from(vec![
			Span::styled(" query ", Style::default().fg(THEME.search.label)),
			raw_marker("ui.search.input#query"),
		]),
		focused,
	)
}

fn search_block_with_title(title: Line<'static>, focused: bool) -> Block<'static> {
	let border = if focused {
		THEME.focus.border
	} else {
		THEME.search.muted
	};
	let bg = if focused {
		THEME.search.focus_bg
	} else {
		THEME.search.background
	};
	Block::default()
		.title(title)
		.borders(Borders::ALL)
		.border_style(Style::default().fg(border).bg(bg))
		.style(Style::default().bg(bg))
}

fn search_query_line(app: &App, width: u16) -> Line<'static> {
	let focused = matches!(app.mode(), UiMode::HeaderSearch(HeaderSearchFocus::Text));
	let raw = app.header_search().text.as_str();
	let width = usize::from(width);
	if focused {
		let value_width = width.saturating_sub(1);
		let value = fit_text(raw, value_width, FitMode::Tail);
		return Line::from(vec![
			Span::styled(value, search_value_style(true)),
			Span::styled(
				"|",
				Style::default()
					.fg(THEME.search.active)
					.bg(THEME.search.focus_bg)
					.add_modifier(Modifier::BOLD),
			),
		]);
	}
	let value = display_filter(raw.trim());
	let fitted = fit_text(value, width, FitMode::Middle);
	Line::from(Span::styled(fitted, search_value_style(false)))
}

fn search_value_style(focused: bool) -> Style {
	if focused {
		Style::default()
			.fg(THEME.search.active)
			.bg(THEME.search.focus_bg)
			.add_modifier(Modifier::BOLD)
	} else {
		Style::default()
			.fg(THEME.search.value)
			.bg(THEME.search.background)
	}
}

fn render_search_popup(frame: &mut ratatui::Frame<'_>, search_area: Rect, app: &App) {
	if !app.header_search().combo_open {
		return;
	}
	let Some((anchor, title, items, cursor)) = search_popup_model(search_area, app) else {
		return;
	};
	if items.is_empty() {
		return;
	}
	let frame_area = frame.area();
	let popup_y = search_area.y.saturating_add(search_area.height);
	if popup_y >= frame_area.height {
		return;
	}
	let width = anchor
		.width
		.max(28)
		.min(frame_area.width.saturating_sub(anchor.x));
	let wanted_height = (items.len() as u16).saturating_add(2).min(8);
	let height = wanted_height.min(frame_area.height.saturating_sub(popup_y));
	let popup = Rect::new(anchor.x, popup_y, width, height);
	let list_items = items
		.into_iter()
		.enumerate()
		.map(|(idx, item)| {
			let style = if idx == cursor {
				Style::default()
					.fg(THEME.search.active)
					.bg(THEME.search.focus_bg)
					.add_modifier(Modifier::BOLD)
			} else {
				Style::default()
					.fg(THEME.search.value)
					.bg(THEME.search.background)
			};
			ListItem::new(Line::from(Span::styled(item, style))).style(style)
		})
		.collect::<Vec<_>>();
	let list = List::new(list_items).block(
		Block::default()
			.title(Span::styled(
				format!(" {title} "),
				Style::default().fg(THEME.search.label),
			))
			.borders(Borders::ALL)
			.border_style(Style::default().fg(THEME.focus.border))
			.style(Style::default().bg(THEME.search.background)),
	);
	frame.render_widget(Clear, popup);
	frame.render_widget(list, popup);
}

fn search_popup_model(
	search_area: Rect,
	app: &App,
) -> Option<(Rect, &'static str, Vec<String>, usize)> {
	let inner = Block::default().borders(Borders::ALL).inner(search_area);
	let regions = search_regions(app, inner);
	let search = app.header_search();
	match app.mode() {
		UiMode::HeaderSearch(HeaderSearchFocus::Lang) => {
			let options = app.available_header_langs();
			let mut items = vec![if search.langs.is_empty() {
				"[x] all languages".to_string()
			} else {
				"clear language filter".to_string()
			}];
			for lang in &options {
				let mark = if search.langs.contains(lang) {
					"[x]"
				} else {
					"[ ]"
				};
				items.push(format!("{mark} {}", lang.tag()));
			}
			Some((regions.lang, "lang selector", items, search.lang_cursor))
		}
		UiMode::HeaderSearch(HeaderSearchFocus::Kind) => {
			let options = app.available_header_kind_filters();
			let mut items = vec![if search.kind_filters.is_empty() {
				"[x] all kinds".to_string()
			} else {
				"clear kind filter".to_string()
			}];
			for option in &options {
				let mark = if search.kind_filters.contains(option) {
					"[x]"
				} else {
					"[ ]"
				};
				items.push(format!("{mark} {}", option.label()));
			}
			Some((regions.kind, "kind selector", items, search.kind_cursor))
		}
		_ => None,
	}
}

fn search_lang_summary(app: &App) -> String {
	lang_filter_summary(&app.header_search().langs)
}

fn search_kind_summary(app: &App) -> String {
	kind_filter_summary(&app.header_search().kind_filters)
}

#[cfg(test)]
fn fit_search_value_widths(available: usize, requested: [usize; 3]) -> [usize; 3] {
	if requested.iter().sum::<usize>() <= available {
		return requested;
	}
	let mut widths = [0; 3];
	let mut remaining = available;
	while remaining > 0
		&& widths
			.iter()
			.zip(requested)
			.any(|(width, max)| *width < max)
	{
		for idx in [1, 2, 0] {
			if remaining == 0 {
				break;
			}
			if widths[idx] < requested[idx] {
				widths[idx] += 1;
				remaining -= 1;
			}
		}
	}
	widths
}

fn render_body(frame: &mut ratatui::Frame<'_>, area: Rect, app: &mut App) {
	let cols = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
		.split(area);
	render_left_pane(frame, cols[0], app);
	let panel = ExplorerFeature::active_panel(app);
	let panel_focused = focus_region_visible(app, FocusRegion::Panel);
	panels::render_panel(
		frame,
		cols[1],
		&panel,
		PanelRenderState {
			scroll: app.panel_scroll(),
			selected: app.selected_panel_item(),
			focused: panel_focused,
		},
	);
}

pub(super) fn focus_region_visible(app: &App, region: FocusRegion) -> bool {
	matches!(app.mode(), UiMode::Normal) && app.focus_region() == region
}

fn render_left_pane(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	if app.usage_lens().is_none() {
		render_primary_nav_list(frame, area, app);
		return;
	}
	let rows = Layout::default()
		.direction(Direction::Vertical)
		.constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
		.split(area);
	render_primary_nav_list(frame, rows[0], app);
	render_usage_nav_list(frame, rows[1], app);
}

#[cfg(test)]
pub(super) fn search_input_visible(app: &App) -> bool {
	let _ = app;
	true
}

#[cfg(test)]
pub(super) fn search_input_value(app: &App) -> String {
	app.header_search().text.clone()
}

#[cfg(test)]
pub(super) fn search_input_title(app: &App) -> String {
	match app.mode() {
		UiMode::HeaderSearch(HeaderSearchFocus::Text) => "search text focused".to_string(),
		UiMode::HeaderSearch(HeaderSearchFocus::Lang) => "search language focused".to_string(),
		UiMode::HeaderSearch(HeaderSearchFocus::Kind) => "search kind focused".to_string(),
		UiMode::Normal => "search".to_string(),
	}
}

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let prefix = match app.mode() {
		UiMode::HeaderSearch(HeaderSearchFocus::Text) => "search",
		UiMode::HeaderSearch(HeaderSearchFocus::Lang) => "lang",
		UiMode::HeaderSearch(HeaderSearchFocus::Kind) => "kind",
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

fn render_primary_nav_list(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
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
	} else {
		format!(
			" navigator {} files {} defs ",
			app.store().stats().files,
			app.app_store.navigation().explorer_def_count()
		)
	};
	render_nav_block(
		frame,
		area,
		app,
		title,
		ComponentId::Navigator,
		app.nav_rows(),
		app.selected_nav_index(),
		app.primary_expanded(),
		focus_region_visible(app, FocusRegion::Navigator),
	);
}

fn render_usage_nav_list(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let Some(focus) = app.usage_lens() else {
		return;
	};
	let title = format!(
		" usages {}  {} files {} defs ",
		focus.label,
		matched_file_count(&focus.contexts),
		focus.contexts.len()
	);
	render_nav_block(
		frame,
		area,
		app,
		title,
		ComponentId::NavigatorUsages,
		app.usage_nav_rows(),
		app.selected_usage_nav_index(),
		app.usage_expanded(),
		focus_region_visible(app, FocusRegion::UsageLens),
	);
}

fn render_nav_block(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	app: &App,
	title: String,
	component: ComponentId,
	rows: &[NavRow],
	selection: usize,
	expanded: &BTreeSet<super::store::ids::NodeId>,
	focused: bool,
) {
	let block = Block::default()
		.title(focused_block_title(title, component, focused))
		.borders(Borders::ALL)
		.border_style(if focused {
			Style::default().fg(THEME.focus.border)
		} else {
			Style::default()
		});
	let inner = block.inner(area);
	let visible_rows = inner.height as usize;
	let viewport = ScrollViewport::for_selection_with_margin(
		rows.len(),
		visible_rows,
		selection,
		viewport_comfort_margin(visible_rows),
	);
	let content_area = viewport.content_area(inner);
	let start = viewport.offset;
	let end = (start + visible_rows).min(rows.len());
	let items: Vec<ListItem<'_>> = rows[start..end]
		.iter()
		.enumerate()
		.map(|(offset, row)| {
			let idx = start + offset;
			let line = nav_row_line(app, row, idx == selection, expanded);
			let style = if idx == selection {
				Style::default().bg(THEME.nav.selected_bg)
			} else {
				Style::default()
			};
			ListItem::new(line).style(style)
		})
		.collect();
	frame.render_widget(block, area);
	frame.render_widget(List::new(items), content_area);
	render_vertical_scrollbar(frame, inner, viewport);
}

pub(super) fn nav_row_line(
	app: &App,
	row: &NavRow,
	selected: bool,
	expanded: &BTreeSet<super::store::ids::NodeId>,
) -> Line<'static> {
	let marker = if selected { ">" } else { " " };
	let indent = "  ".repeat(row.depth);
	let twisty = if row.has_children {
		if expanded.contains(&row.key) {
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
			let symbol = app.store().symbol_summary(&loc);
			let group = definition_kind_group(symbol.lang, &symbol.kind);
			spans.push(Span::styled(
				symbol.kind,
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
			if let Some(change) = symbol.change {
				spans.push(Span::raw("  "));
				spans.push(change_marker_span(change.status));
				spans.push(Span::styled(
					format!("  {} usages", change.usage_count),
					Style::default().fg(THEME.nav.meta),
				));
			}
		}
		NavNodeKind::Change(id) => {
			let Some(change) = app.store().change_summary(id) else {
				return Line::from(spans);
			};
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
			spans.push(Span::styled(
				format!("  {} usages", change.usage_count),
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

#[cfg(test)]
pub(super) fn active_panel_snapshot(app: &App) -> panels::PanelSnapshot {
	let panel = ExplorerFeature::active_panel(app);
	panels::panel_snapshot(&panel, DEFAULT_PANEL_SNAPSHOT_WIDTH)
}

#[cfg(test)]
pub(super) fn change_panel_lines(app: &App, width: usize) -> Vec<Line<'static>> {
	let panel = ExplorerFeature::active_panel(app);
	panels::panel_snapshot(&panel, width).lines
}

#[cfg(test)]
pub(super) fn refs_panel_lines(app: &App, loc: DefLocation, width: usize) -> Vec<Line<'static>> {
	let panel = ExplorerFeature::refs_for_symbol_panel(app, loc);
	panels::panel_snapshot(&panel, width).lines
}

pub(super) fn current_panel_snapshot_width() -> usize {
	crossterm::terminal::size()
		.map(|(width, height)| {
			let area = Rect::new(0, 0, width, height);
			let rows = Layout::default()
				.direction(Direction::Vertical)
				.constraints([
					Constraint::Length(1),
					Constraint::Length(SEARCH_WIDGET_HEIGHT),
					Constraint::Min(0),
					Constraint::Length(1),
				])
				.split(area);
			let cols = Layout::default()
				.direction(Direction::Horizontal)
				.constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
				.split(rows[2]);
			panel::content_width(cols[1])
		})
		.unwrap_or(DEFAULT_PANEL_SNAPSHOT_WIDTH)
}
