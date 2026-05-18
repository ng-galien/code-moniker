use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::super::component::{block_title, marker};
use super::super::kinds::reference_kind_group;
use super::super::panel;
use super::super::scroll::{ScrollViewport, render_vertical_scrollbar};
use super::super::source::SourceLineVm;
use super::super::text::{FitMode, fit_text, visible_len};
use super::super::theme::{SourceTheme, THEME};
use super::model::{MessageTone, PanelSection, PanelVm, ReferenceGroupVm, WrapMode};

#[derive(Clone, Debug)]
pub(in crate::ui) struct PanelSnapshot {
	pub(in crate::ui) title: &'static str,
	pub(in crate::ui) component: super::super::component::ComponentId,
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
	scroll_offset: usize,
) {
	let block = Block::default()
		.title(block_title(panel.title, panel.component))
		.borders(Borders::ALL);
	let inner = block.inner(area);
	let width = content_width(inner);
	let initial_lines = panel_lines(panel, width);
	let initial_viewport = ScrollViewport::from_offset(
		initial_lines.len(),
		usize::from(inner.height),
		scroll_offset,
	);
	let content_area = initial_viewport.content_area(inner);
	let width = content_width(content_area);
	let lines = if content_area.width == inner.width {
		initial_lines
	} else {
		panel_lines(panel, width)
	};
	let viewport =
		ScrollViewport::from_offset(lines.len(), usize::from(inner.height), scroll_offset);
	let paragraph = Paragraph::new(Text::from(lines)).scroll((viewport.offset_u16(), 0));
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
		lines: panel_lines(panel, width),
	}
}

fn panel_lines(panel: &PanelVm, width: usize) -> Vec<Line<'static>> {
	let mut lines = Vec::new();
	for section in &panel.sections {
		match section {
			PanelSection::Heading { label } => lines.push(panel::section(label.clone())),
			PanelSection::ComponentHeading { label, component } => {
				lines.push(Line::from(vec![
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
				lines.push(panel::kv(label, value, width, *fit));
			}
			PanelSection::Table { columns, rows } => {
				lines.push(panel::table_header(columns, width));
				lines.push(panel::separator(panel::table_width(columns, width)));
				for row in rows {
					lines.push(panel::table_row(columns, row, width));
				}
			}
			PanelSection::Message { text, tone } => lines.push(match tone {
				MessageTone::Muted => panel::muted(text.clone()),
				MessageTone::Danger => {
					Line::styled(text.clone(), Style::default().fg(THEME.danger))
				}
			}),
			PanelSection::Bullet { text } => lines.push(panel::bullet(text.clone())),
			PanelSection::SourceSnippet(snippet) => {
				lines.extend(source_snippet_lines(snippet));
			}
			PanelSection::ReferenceGroups { groups, limit } => {
				push_ref_groups(&mut lines, groups, *limit, width);
			}
			PanelSection::Blank => lines.push(panel::blank()),
		}
	}
	lines
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
	lines: &mut Vec<Line<'static>>,
	groups: &[ReferenceGroupVm],
	limit: usize,
	width: usize,
) {
	if groups.is_empty() {
		lines.push(panel::muted("none"));
		return;
	}
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
