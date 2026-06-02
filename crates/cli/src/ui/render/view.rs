use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use code_moniker_workspace::snapshot::ChangeStatus;

use super::super::DEFAULT_PANEL_SNAPSHOT_WIDTH;
use super::super::events::HeaderSearchFocus;
use super::super::explorer::{
	ExplorerVm, FooterVm, HeaderVm, NavPaneVm, NavRowVm, NavRowVmKind, SearchBarVm, SearchPopupVm,
};
use super::super::panel;
use super::super::panel::PanelRenderState;
use super::component::{ComponentId, focused_block_title, marker, raw_marker};
use super::kinds::definition_kind_group;
use super::scroll::{ScrollViewport, render_vertical_scrollbar, viewport_comfort_margin};
use super::text::{FitMode, fit_text, visible_len};
use super::theme::THEME;
use super::tree;

const SEARCH_WIDGET_HEIGHT: u16 = 5;

pub(in crate::ui) fn render_shell(frame: &mut ratatui::Frame<'_>, area: Rect, vm: &ExplorerVm) {
	let rows = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(1),
			Constraint::Length(SEARCH_WIDGET_HEIGHT),
			Constraint::Min(0),
			Constraint::Length(1),
		])
		.split(area);
	render_header(frame, rows[0], &vm.header, vm.show_component_markers);
	render_search_bar(frame, rows[1], &vm.search, vm.show_component_markers);
	render_body(frame, rows[2], vm);
	render_footer(frame, rows[3], &vm.footer, vm.show_component_markers);
	render_search_popup(frame, rows[1], &vm.search);
}

fn render_header(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	header: &HeaderVm,
	show_component_markers: bool,
) {
	frame.render_widget(
		Paragraph::new(header_line(
			header,
			usize::from(area.width),
			show_component_markers,
		)),
		area,
	);
}

pub(in crate::ui) fn header_line(
	header: &HeaderVm,
	width: usize,
	show_component_markers: bool,
) -> Line<'static> {
	let prefix_width = visible_len("code-moniker ")
		+ if show_component_markers {
			visible_len(ComponentId::Header.as_str()) + 2
		} else {
			0
		} + visible_len(" mode ")
		+ visible_len(header.mode)
		+ visible_len("  scope ");
	let scope = fit_text(
		&header.scope,
		width.saturating_sub(prefix_width),
		FitMode::Middle,
	);
	let mut spans = vec![Span::styled(
		"code-moniker ",
		Style::default()
			.fg(THEME.brand)
			.add_modifier(Modifier::BOLD),
	)];
	if show_component_markers {
		spans.push(marker(ComponentId::Header));
		spans.push(Span::raw(" "));
	}
	spans.extend([
		Span::raw("mode "),
		Span::styled(
			header.mode,
			Style::default()
				.fg(THEME.section)
				.add_modifier(Modifier::BOLD),
		),
		Span::raw("  scope "),
		Span::styled(scope, Style::default().fg(THEME.nav.symbol)),
	]);
	Line::from(spans)
}

fn render_search_bar(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	search: &SearchBarVm,
	show_component_markers: bool,
) {
	let block = Block::default()
		.title(focused_block_title(
			" search ",
			ComponentId::SearchInput,
			search.focused,
			show_component_markers,
		))
		.borders(Borders::ALL)
		.border_style(if search.focused {
			Style::default().fg(THEME.focus.border)
		} else {
			Style::default()
		})
		.style(Style::default().bg(THEME.search.background));
	let inner = block.inner(area);
	frame.render_widget(block, area);
	let regions = search_regions(search, inner);
	render_search_query(frame, regions.query, search, show_component_markers);
	render_search_combo(
		frame,
		regions.lang,
		"lang",
		search.lang_summary.clone(),
		search,
		HeaderSearchFocus::Lang,
	);
	render_search_combo(
		frame,
		regions.kind,
		"kind",
		search.kind_summary.clone(),
		search,
		HeaderSearchFocus::Kind,
	);
}

#[derive(Copy, Clone, Debug)]
struct SearchRegions {
	query: Rect,
	lang: Rect,
	kind: Rect,
}

fn search_regions(search: &SearchBarVm, area: Rect) -> SearchRegions {
	let lang_width = combo_width("lang", &search.lang_summary, 16, 30);
	let kind_width = combo_width("kind", &search.kind_summary, 18, 38);
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

fn render_search_query(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	search: &SearchBarVm,
	show_component_markers: bool,
) {
	let focused = search.focus == Some(HeaderSearchFocus::Text);
	let block = search_query_block(focused, show_component_markers);
	let inner = block.inner(area);
	frame.render_widget(block, area);
	frame.render_widget(
		Paragraph::new(search_query_line(search, inner.width)),
		inner,
	);
}

fn render_search_combo(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	label: &'static str,
	value: String,
	search: &SearchBarVm,
	focus: HeaderSearchFocus,
) {
	let focused = search.focus == Some(focus);
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

fn search_query_block(focused: bool, show_component_markers: bool) -> Block<'static> {
	let mut spans = vec![Span::styled(
		" query ",
		Style::default().fg(THEME.search.label),
	)];
	if show_component_markers {
		spans.push(raw_marker("ui.search.input#query"));
	}
	search_block_with_title(Line::from(spans), focused)
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

fn search_query_line(search: &SearchBarVm, width: u16) -> Line<'static> {
	let focused = search.focus == Some(HeaderSearchFocus::Text);
	let width = usize::from(width);
	if focused {
		let value_width = width.saturating_sub(1);
		let value = fit_text(&search.text, value_width, FitMode::Tail);
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
	let fitted = fit_text(&search.display_text, width, FitMode::Middle);
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

fn render_search_popup(frame: &mut ratatui::Frame<'_>, search_area: Rect, search: &SearchBarVm) {
	if !search.combo_open {
		return;
	}
	let Some((anchor, popup)) = search_popup_model(search_area, search) else {
		return;
	};
	if popup.items.is_empty() {
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
	let wanted_height = (popup.items.len() as u16).saturating_add(2).min(8);
	let height = wanted_height.min(frame_area.height.saturating_sub(popup_y));
	let popup_area = Rect::new(anchor.x, popup_y, width, height);
	let list_items = popup
		.items
		.iter()
		.enumerate()
		.map(|(idx, item)| {
			let style = if idx == popup.cursor {
				Style::default()
					.fg(THEME.search.active)
					.bg(THEME.search.focus_bg)
					.add_modifier(Modifier::BOLD)
			} else {
				Style::default()
					.fg(THEME.search.value)
					.bg(THEME.search.background)
			};
			ListItem::new(Line::from(Span::styled(item.clone(), style))).style(style)
		})
		.collect::<Vec<_>>();
	let list = List::new(list_items).block(
		Block::default()
			.title(Span::styled(
				format!(" {} ", popup.title),
				Style::default().fg(THEME.search.label),
			))
			.borders(Borders::ALL)
			.border_style(Style::default().fg(THEME.focus.border))
			.style(Style::default().bg(THEME.search.background)),
	);
	frame.render_widget(Clear, popup_area);
	frame.render_widget(list, popup_area);
}

fn search_popup_model(search_area: Rect, search: &SearchBarVm) -> Option<(Rect, &SearchPopupVm)> {
	let popup = search.popup.as_ref()?;
	let inner = Block::default().borders(Borders::ALL).inner(search_area);
	let regions = search_regions(search, inner);
	let anchor = match popup.focus {
		HeaderSearchFocus::Lang => regions.lang,
		HeaderSearchFocus::Kind => regions.kind,
		HeaderSearchFocus::Text => return None,
	};
	Some((anchor, popup))
}

fn render_body(frame: &mut ratatui::Frame<'_>, area: Rect, vm: &ExplorerVm) {
	let cols = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
		.split(area);
	render_left_pane(frame, cols[0], vm);
	panel::render_panel(
		frame,
		cols[1],
		&vm.panel,
		PanelRenderState {
			scroll: vm.panel_navigation.scroll,
			selected: vm.panel_navigation.selected,
			focused: vm.panel_focused,
			show_component_markers: vm.show_component_markers,
		},
	);
}

fn render_left_pane(frame: &mut ratatui::Frame<'_>, area: Rect, vm: &ExplorerVm) {
	let Some(usage_nav) = &vm.usage_nav else {
		render_nav_block(frame, area, &vm.primary_nav, vm.show_component_markers);
		return;
	};
	let rows = Layout::default()
		.direction(Direction::Vertical)
		.constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
		.split(area);
	render_nav_block(frame, rows[0], &vm.primary_nav, vm.show_component_markers);
	render_nav_block(frame, rows[1], usage_nav, vm.show_component_markers);
}

fn render_footer(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	footer: &FooterVm,
	show_component_markers: bool,
) {
	let mut spans = vec![Span::styled(
		format!("{}: ", footer.prefix),
		Style::default().fg(THEME.status_label),
	)];
	if show_component_markers {
		spans.push(marker(ComponentId::Status));
		spans.push(Span::raw(" "));
	}
	spans.push(Span::raw(footer.status.clone()));
	let line = Line::from(spans);
	frame.render_widget(Paragraph::new(line), area);
}

fn render_nav_block(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	pane: &NavPaneVm,
	show_component_markers: bool,
) {
	let block = Block::default()
		.title(focused_block_title(
			pane.title.clone(),
			pane.component,
			pane.focused,
			show_component_markers,
		))
		.borders(Borders::ALL)
		.border_style(if pane.focused {
			Style::default().fg(THEME.focus.border)
		} else {
			Style::default()
		});
	let inner = block.inner(area);
	let visible_rows = inner.height as usize;
	let viewport = ScrollViewport::for_selection_with_margin(
		pane.rows.len(),
		visible_rows,
		pane.selection,
		viewport_comfort_margin(visible_rows),
	);
	let content_area = viewport.content_area(inner);
	let start = viewport.offset;
	let end = (start + visible_rows).min(pane.rows.len());
	let items: Vec<ListItem<'_>> = pane.rows[start..end]
		.iter()
		.enumerate()
		.map(|(offset, row)| {
			let idx = start + offset;
			let line = nav_row_line(row, idx == pane.selection);
			let style = if idx == pane.selection {
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

pub(in crate::ui) fn nav_row_line(row: &NavRowVm, selected: bool) -> Line<'static> {
	let mut spans = tree::prefix_spans(row.depth, row.has_children, row.expanded, Some(selected));
	push_view_marker(row, &mut spans);
	match &row.kind {
		NavRowVmKind::Lang => {
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
		NavRowVmKind::Dir => {
			spans.push(Span::styled(
				format!("{}/", row.label),
				Style::default().fg(THEME.nav.directory),
			));
			spans.push(nav_count_span(row));
		}
		NavRowVmKind::File { change_count } => {
			spans.push(Span::styled(
				row.label.clone(),
				Style::default()
					.fg(THEME.nav.file)
					.add_modifier(Modifier::BOLD),
			));
			spans.push(nav_count_span(row));
			if let Some(count) = change_count {
				spans.push(change_count_span(*count));
			}
		}
		NavRowVmKind::ChangeFile => {
			spans.push(Span::styled(
				row.label.clone(),
				Style::default()
					.fg(THEME.nav.file)
					.add_modifier(Modifier::BOLD),
			));
			spans.push(nav_count_span(row));
		}
		NavRowVmKind::View { scope, .. } => push_view_row_spans(row, scope, &mut spans),
		NavRowVmKind::ViewError => {
			spans.push(Span::styled(
				row.label.clone(),
				Style::default().fg(THEME.danger),
			));
		}
		NavRowVmKind::Def {
			lang,
			kind,
			visibility,
			change,
		} => {
			let group = definition_kind_group(*lang, kind);
			spans.push(Span::styled(
				kind.clone(),
				Style::default().fg(THEME.kind.color_for_group(group)),
			));
			if !visibility.is_empty() {
				spans.push(Span::raw(" "));
				spans.push(Span::styled(
					visibility.clone(),
					Style::default().fg(THEME.nav.visibility),
				));
			}
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
			if let Some(change) = change {
				spans.push(Span::raw("  "));
				spans.push(change_marker_span(change.status));
				spans.push(Span::styled(
					format!("  {} usages", change.usage_count),
					Style::default().fg(THEME.nav.meta),
				));
			}
		}
		NavRowVmKind::Change(Some(change)) => {
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
		NavRowVmKind::Change(None) | NavRowVmKind::Root => {}
	}
	Line::from(spans)
}

fn push_view_row_spans(row: &NavRowVm, scope: &str, spans: &mut Vec<Span<'static>>) {
	spans.push(Span::styled(
		"view",
		Style::default()
			.fg(THEME.section)
			.add_modifier(Modifier::BOLD),
	));
	spans.push(Span::raw(" "));
	spans.push(Span::styled(
		row.label.clone(),
		Style::default().fg(THEME.nav.symbol),
	));
	spans.push(Span::styled(
		format!("  workspace/views/{}", row.label),
		Style::default().fg(THEME.nav.meta),
	));
	if !scope.is_empty() {
		spans.push(Span::styled(
			format!("  scope {scope}"),
			Style::default().fg(THEME.nav.directory),
		));
	}
}

fn push_view_marker(row: &NavRowVm, spans: &mut Vec<Span<'static>>) {
	if row.view_count == 0
		|| matches!(
			row.kind,
			NavRowVmKind::View { .. } | NavRowVmKind::ViewError
		) {
		return;
	}
	spans.push(Span::styled(
		format!("[v{}] ", row.view_count),
		Style::default()
			.fg(THEME.section)
			.add_modifier(Modifier::BOLD),
	));
}

fn nav_count_span(row: &NavRowVm) -> Span<'static> {
	let reexports = if row.def_count == 0 && row.reexport_count > 0 {
		format!("  {} reexports", row.reexport_count)
	} else {
		String::new()
	};
	let label = match (row.file_count, row.def_count) {
		(0, defs) => format!("  {defs} defs{reexports}"),
		(files, defs) => format!("  {files} files  {defs} defs{reexports}"),
	};
	Span::styled(label, Style::default().fg(THEME.nav.meta))
}

fn change_count_span(count: usize) -> Span<'static> {
	Span::styled(
		format!("  {count} change(s)"),
		Style::default().fg(THEME.change_modified),
	)
}

fn change_marker_span(status: ChangeStatus) -> Span<'static> {
	Span::styled(
		change_status_marker(status).to_string(),
		Style::default().fg(change_status_color(status)),
	)
}

fn change_status_marker(status: ChangeStatus) -> &'static str {
	match status {
		ChangeStatus::Added => "+",
		ChangeStatus::Modified => "~",
		ChangeStatus::Removed => "-",
	}
}

fn change_status_color(status: ChangeStatus) -> ratatui::style::Color {
	match status {
		ChangeStatus::Added => THEME.change_added,
		ChangeStatus::Modified => THEME.change_modified,
		ChangeStatus::Removed => THEME.danger,
	}
}

pub(in crate::ui) fn current_panel_snapshot_width() -> usize {
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
