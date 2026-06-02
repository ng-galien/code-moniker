use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::render::text::{self, Column, FitMode, fit_text, format_cell, visible_len};
use super::render::theme::THEME;

mod model;
mod render;

pub(in crate::ui) use model::{
	PanelRenderState, PanelVm, ReferenceGroupVm, SourceLineVm, panel_blank, panel_bullet,
	panel_component_section, panel_danger, panel_evidence, panel_info, panel_kv, panel_muted,
	panel_reference_groups, panel_section, panel_source_snippet, panel_table, panel_tree_rows,
	panel_warning,
};
pub(in crate::ui) use render::PanelSnapshot;

pub(super) fn render_panel(
	frame: &mut ratatui::Frame<'_>,
	area: Rect,
	panel: &PanelVm,
	state: PanelRenderState,
) {
	render::render_panel_vm(frame, area, panel, state);
}

pub(super) fn panel_snapshot(panel: &PanelVm, width: usize) -> PanelSnapshot {
	render::snapshot(panel, width)
}

pub(super) fn section(label: impl Into<String>) -> Line<'static> {
	Line::styled(
		label.into(),
		Style::default()
			.fg(THEME.panel.section)
			.add_modifier(Modifier::BOLD),
	)
}

pub(super) fn separator(width: usize) -> Line<'static> {
	let width = width.clamp(12, 96);
	Line::styled(
		"-".repeat(width),
		Style::default().fg(THEME.panel.separator),
	)
}

pub(super) fn blank() -> Line<'static> {
	Line::raw("")
}

pub(super) fn muted(text: impl Into<String>) -> Line<'static> {
	Line::styled(text.into(), Style::default().fg(THEME.panel.muted))
}

pub(super) fn bullet(text: impl Into<String>) -> Line<'static> {
	Line::from(vec![
		Span::styled("  - ", Style::default().fg(THEME.panel.muted)),
		Span::styled(text.into(), Style::default().fg(THEME.panel.value)),
	])
}

pub(super) fn kv(label: &str, value: &str, width: usize, mode: FitMode) -> Line<'static> {
	let prefix = format!("{label:<10} ");
	let value_width = width.saturating_sub(visible_len(&prefix));
	Line::from(vec![
		Span::styled(prefix, Style::default().fg(THEME.panel.label)),
		Span::styled(
			fit_text(value, value_width, mode),
			Style::default().fg(THEME.panel.value),
		),
	])
}

pub(super) fn table_width(columns: &[Column], max_width: usize) -> usize {
	text::table_width(columns, max_width)
}

pub(super) fn table_header(columns: &[Column], max_width: usize) -> Line<'static> {
	let widths = text::fitted_widths(columns, max_width);
	let spans = columns
		.iter()
		.zip(widths)
		.enumerate()
		.flat_map(|(idx, (column, width))| {
			let mut spans = Vec::new();
			if idx > 0 {
				spans.push(Span::raw("  "));
			}
			spans.push(Span::styled(
				format_cell(column.title, width, column.align),
				Style::default()
					.fg(THEME.panel.header)
					.add_modifier(Modifier::BOLD),
			));
			spans
		})
		.collect::<Vec<_>>();
	Line::from(spans)
}

pub(super) fn table_row(columns: &[Column], values: &[String], max_width: usize) -> Line<'static> {
	let widths = text::fitted_widths(columns, max_width);
	let spans = columns
		.iter()
		.zip(widths)
		.enumerate()
		.flat_map(|(idx, (column, width))| {
			let value = values.get(idx).map(String::as_str).unwrap_or("");
			let mut spans = Vec::new();
			if idx > 0 {
				spans.push(Span::raw("  "));
			}
			spans.push(Span::styled(
				format_cell(value, width, column.align),
				Style::default().fg(THEME.panel.value),
			));
			spans
		})
		.collect::<Vec<_>>();
	Line::from(spans)
}

pub(super) fn content_width(area: Rect) -> usize {
	usize::from(area.width.saturating_sub(2)).max(20)
}
