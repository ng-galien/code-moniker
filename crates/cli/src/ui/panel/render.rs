use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super as panel;
use super::super::render::component::{focused_block_title, marker};
use super::super::render::kinds::reference_kind_group;
use super::super::render::scroll::{
	ScrollViewport, render_vertical_scrollbar, viewport_comfort_margin,
};
use super::super::render::text::{FitMode, fit_text, visible_len};
use super::super::render::theme::{SourceTheme, THEME};
use super::super::render::tree;
use super::SourceLineVm;
use super::model::{
	MessageTone, PanelRenderState, PanelSection, PanelVm, ReferenceGroupVm, WrapMode,
};

#[derive(Clone, Debug)]
pub(in crate::ui) struct PanelSnapshot {
	pub(in crate::ui) title: &'static str,
	pub(in crate::ui) component: super::super::render::component::ComponentId,
	pub(in crate::ui) lines: Vec<Line<'static>>,
}

impl PanelSnapshot {
	pub(in crate::ui) fn to_text(&self, mode: &str, scope: &str) -> String {
		let mut lines = vec![
			"code-moniker panel snapshot".to_string(),
			format!("component {}", self.component.as_str()),
			format!("title     {}", self.title),
			format!("mode      {mode}"),
			format!("scope     {scope}"),
			String::new(),
		];
		lines.extend(self.lines.iter().map(plain_line_text));
		lines.join("\n")
	}
}

pub(super) fn render_panel_vm(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	panel: &PanelVm,
	state: PanelRenderState,
) {
	let border_style = if state.focused {
		Style::default().fg(THEME.focus.border)
	} else {
		Style::default()
	};
	let block = Block::default()
		.title(focused_block_title(
			panel.title,
			panel.component,
			state.focused,
		))
		.borders(Borders::ALL)
		.border_style(border_style);
	let inner = block.inner(area);
	let width = content_width(inner);
	let initial_lines = panel_lines(panel, width, state);
	let initial_viewport = ScrollViewport::from_offset(
		initial_lines.lines.len(),
		usize::from(inner.height),
		state.scroll,
	);
	let content_area = initial_viewport.content_area(inner);
	let width = content_width(content_area);
	let lines = if content_area.width == inner.width {
		initial_lines
	} else {
		panel_lines(panel, width, state)
	};
	let selected_line = state
		.selected
		.and_then(|selected| lines.navigable_lines.get(selected).copied());
	let viewport = ScrollViewport::for_visible_line(
		lines.lines.len(),
		usize::from(inner.height),
		state.scroll,
		selected_line,
		viewport_comfort_margin(usize::from(inner.height)),
	);
	let paragraph = Paragraph::new(Text::from(lines.lines)).scroll((viewport.offset_u16(), 0));
	frame.render_widget(block, area);
	match panel.wrap {
		WrapMode::Wrap => frame.render_widget(
			paragraph.wrap(Wrap { trim: false }),
			viewport.content_area(inner),
		),
		WrapMode::NoWrap => frame.render_widget(paragraph, viewport.content_area(inner)),
	}
	render_vertical_scrollbar(frame, inner, viewport);
}

fn content_width(area: Rect) -> usize {
	usize::from(area.width).max(20)
}

pub(super) fn snapshot(panel: &PanelVm, width: usize) -> PanelSnapshot {
	PanelSnapshot {
		title: panel.title,
		component: panel.component,
		lines: panel_lines(panel, width, PanelRenderState::default()).lines,
	}
}

#[derive(Debug)]
struct RenderedPanelLines {
	lines: Vec<Line<'static>>,
	navigable_lines: Vec<usize>,
}

impl RenderedPanelLines {
	fn new() -> Self {
		Self {
			lines: Vec::new(),
			navigable_lines: Vec::new(),
		}
	}

	fn push(&mut self, line: Line<'static>) {
		self.lines.push(line);
	}

	fn push_navigable(&mut self, line: Line<'static>, selected: bool, focused: bool) {
		self.navigable_lines.push(self.lines.len());
		self.lines.push(highlight_line(line, selected, focused));
	}
}

fn panel_lines(panel: &PanelVm, width: usize, state: PanelRenderState) -> RenderedPanelLines {
	let mut rendered = RenderedPanelLines::new();
	let mut nav_idx = 0;
	for section in &panel.sections {
		match section {
			PanelSection::Heading { label } => rendered.push(panel::section(label.clone())),
			PanelSection::ComponentHeading { label, component } => {
				rendered.push(Line::from(vec![
					Span::styled(
						label.clone(),
						Style::default()
							.fg(THEME.panel.section)
							.add_modifier(Modifier::BOLD),
					),
					Span::raw(" "),
					marker(*component),
				]));
			}
			PanelSection::KeyValue { label, value, fit } => {
				rendered.push(panel::kv(label, value, width, *fit));
			}
			PanelSection::Table { columns, rows } => {
				rendered.push(panel::table_header(columns, width));
				rendered.push(panel::separator(panel::table_width(columns, width)));
				for row in rows {
					rendered.push_navigable(
						panel::table_row(columns, row, width),
						state.selected == Some(nav_idx),
						state.focused,
					);
					nav_idx += 1;
				}
			}
			PanelSection::Message { text, tone } => rendered.push(match tone {
				MessageTone::Muted => panel::muted(text.clone()),
				MessageTone::Danger => {
					Line::styled(text.clone(), Style::default().fg(THEME.danger))
				}
			}),
			PanelSection::Bullet { text } => rendered.push(panel::bullet(text.clone())),
			PanelSection::TreeRows(rows) => {
				for row in rows {
					rendered.push_navigable(
						tree::panel_tree_row(row, width),
						state.selected == Some(nav_idx),
						state.focused,
					);
					nav_idx += 1;
				}
			}
			PanelSection::SourceSnippet(snippet) => {
				for line in source_snippet_lines(snippet) {
					rendered.push_navigable(line, state.selected == Some(nav_idx), state.focused);
					nav_idx += 1;
				}
			}
			PanelSection::ReferenceGroups { groups, limit } => {
				push_ref_groups(&mut rendered, groups, *limit, width, state, &mut nav_idx);
			}
			PanelSection::Blank => rendered.push(panel::blank()),
		}
	}
	rendered
}

pub(in crate::ui) fn highlight_line(
	line: Line<'static>,
	selected: bool,
	focused: bool,
) -> Line<'static> {
	if !selected {
		return line;
	}
	let bg = if focused {
		THEME.panel.selected_focus_bg
	} else {
		THEME.panel.selected_bg
	};
	let mut line = line.style(Style::default().bg(bg));
	for span in &mut line.spans {
		span.style = span.style.bg(bg);
	}
	line
}

pub(in crate::ui) fn source_snippet_lines(snippet: &[SourceLineVm]) -> Vec<Line<'static>> {
	let theme = THEME.source;
	snippet
		.iter()
		.map(|line| source_line(line, theme))
		.collect()
}

fn source_line(line: &SourceLineVm, theme: SourceTheme) -> Line<'static> {
	let line_bg = if line.active {
		theme.active_bg
	} else {
		theme.context_bg
	};
	let number_style = if line.active {
		Style::default()
			.fg(theme.active_number_fg)
			.bg(line_bg)
			.add_modifier(Modifier::BOLD)
	} else {
		Style::default().fg(theme.context_number_fg).bg(line_bg)
	};
	let code_style = if line.active {
		Style::default().fg(theme.active_fg).bg(line_bg)
	} else {
		Style::default().fg(theme.context_fg).bg(line_bg)
	};
	let indent_style = if line.active {
		Style::default().bg(theme.active_indent_bg)
	} else {
		Style::default().bg(theme.context_indent_bg)
	};
	let gutter = if line.active { " │ " } else { " ┆ " };
	let (indent, body) = split_leading_ws(&line.text);
	let mut spans = vec![
		Span::styled(
			format!("{:>width$}", line.number, width = line.number_width),
			number_style,
		),
		Span::styled(gutter, Style::default().fg(theme.gutter_fg).bg(line_bg)),
	];
	if !indent.is_empty() {
		spans.push(Span::styled(expand_indent(indent), indent_style));
	}
	spans.push(Span::styled(body.to_string(), code_style));
	Line::from(spans).style(Style::default().bg(line_bg))
}

fn split_leading_ws(text: &str) -> (&str, &str) {
	for (idx, ch) in text.char_indices() {
		if ch != ' ' && ch != '\t' {
			return text.split_at(idx);
		}
	}
	(text, "")
}

fn expand_indent(indent: &str) -> String {
	indent.replace('\t', "    ")
}

fn push_ref_groups(
	lines: &mut RenderedPanelLines,
	groups: &[ReferenceGroupVm],
	limit: usize,
	width: usize,
	state: PanelRenderState,
	nav_idx: &mut usize,
) {
	if groups.is_empty() {
		lines.push(panel::muted("none"));
		return;
	}
	for (idx, group) in groups.iter().take(limit).enumerate() {
		if idx > 0 {
			lines.push(panel::blank());
		}
		let selected = state.selected == Some(*nav_idx);
		for (line_idx, line) in ref_group_lines(group, width).into_iter().enumerate() {
			if line_idx == 0 {
				lines.push_navigable(line, selected, state.focused);
			} else {
				lines.push(highlight_line(line, selected, state.focused));
			}
		}
		*nav_idx += 1;
	}
	if groups.len() > limit {
		lines.push(panel::muted(format!("... {} more", groups.len() - limit)));
	}
}

fn ref_group_lines(group: &ReferenceGroupVm, width: usize) -> Vec<Line<'static>> {
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
	let actor_width = width.saturating_sub(visible_len(prefix) + visible_len(&suffix));
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

fn ref_attrs_line(group: &ReferenceGroupVm, width: usize) -> Option<Line<'static>> {
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

fn plain_line_text(line: &Line<'_>) -> String {
	line.spans
		.iter()
		.map(|span| span.content.as_ref())
		.collect()
}
