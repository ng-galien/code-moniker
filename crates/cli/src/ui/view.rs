use std::collections::BTreeSet;

use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::workspace::{ChangeStatus, DefLocation, IndexStore};

use super::app::{ActiveFilter, VisualizationMode};
use super::component::{ComponentId, block_title, marker};
use super::contracts::{RenderContext, Screen};
use super::events::UiMode;
use super::features::explorer::ExplorerFeature;
use super::kinds::definition_kind_group;
use super::navigator::{NavNodeKind, NavRow};
use super::panel;
use super::panels;
use super::text::{FitMode, fit_text, visible_len};
use super::theme::THEME;
use super::{App, DEFAULT_PANEL_SNAPSHOT_WIDTH, display_filter};

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

fn render_body(frame: &mut ratatui::Frame<'_>, area: Rect, app: &mut App) {
	let cols = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
		.split(area);
	render_left_pane(frame, cols[0], app);
	let panel = ExplorerFeature::active_panel(app);
	panels::render_panel(frame, cols[1], &panel);
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

pub(super) fn search_input_visible(app: &App) -> bool {
	app.mode() == UiMode::EditingSearch
		|| matches!(app.active_filter(), ActiveFilter::Search { .. })
}

pub(super) fn search_input_value(app: &App) -> String {
	if app.mode() == UiMode::EditingSearch {
		return app.search_draft().to_string();
	}
	match app.active_filter() {
		ActiveFilter::Search { raw, .. } => raw.clone(),
		_ => String::new(),
	}
}

pub(super) fn search_input_title(app: &App) -> String {
	if app.mode() == UiMode::EditingSearch {
		"search focused".to_string()
	} else {
		"search".to_string()
	}
}

fn render_search_input(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
	let focused = app.mode() == UiMode::EditingSearch;
	let value = search_input_value(app);
	let width = panel::content_width(area);
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

pub(super) fn nav_row_line(app: &App, row: &NavRow, selected: bool) -> Line<'static> {
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
					Constraint::Min(0),
					Constraint::Length(1),
				])
				.split(area);
			let cols = Layout::default()
				.direction(Direction::Horizontal)
				.constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
				.split(rows[1]);
			panel::content_width(cols[1])
		})
		.unwrap_or(DEFAULT_PANEL_SNAPSHOT_WIDTH)
}
