use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme::THEME;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum FitMode {
	Middle,
	Tail,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum Align {
	Left,
	Right,
}

#[derive(Copy, Clone, Debug)]
pub(super) struct Column {
	pub(super) title: &'static str,
	pub(super) width: usize,
	pub(super) align: Align,
}

impl Column {
	pub(super) const fn left(title: &'static str, width: usize) -> Self {
		Self {
			title,
			width,
			align: Align::Left,
		}
	}

	pub(super) const fn right(title: &'static str, width: usize) -> Self {
		Self {
			title,
			width,
			align: Align::Right,
		}
	}
}

pub(super) fn section(label: impl Into<String>) -> Line<'static> {
	Line::styled(
		label.into(),
		Style::default()
			.fg(THEME.panel.section)
			.add_modifier(Modifier::BOLD),
	)
}

pub(super) fn danger_section(label: impl Into<String>) -> Line<'static> {
	Line::styled(
		label.into(),
		Style::default()
			.fg(THEME.danger)
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
	let prefix = format!("{label:<10}");
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
	let widths = fitted_widths(columns, max_width);
	widths.iter().sum::<usize>() + gap_width(columns)
}

pub(super) fn table_header(columns: &[Column], max_width: usize) -> Line<'static> {
	let widths = fitted_widths(columns, max_width);
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
	let widths = fitted_widths(columns, max_width);
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

fn fitted_widths(columns: &[Column], max_width: usize) -> Vec<usize> {
	if columns.is_empty() {
		return Vec::new();
	}
	let gaps = gap_width(columns);
	let available = max_width.saturating_sub(gaps);
	let requested: Vec<_> = columns.iter().map(|column| column.width).collect();
	if requested.iter().sum::<usize>() <= available {
		return requested;
	}
	let mut widths = vec![0; columns.len()];
	let mut remaining = available;
	while remaining > 0
		&& widths
			.iter()
			.zip(&requested)
			.any(|(width, max)| width < max)
	{
		for (width, max) in widths.iter_mut().zip(&requested) {
			if remaining == 0 {
				break;
			}
			if *width < *max {
				*width += 1;
				remaining -= 1;
			}
		}
	}
	widths
}

fn gap_width(columns: &[Column]) -> usize {
	columns.len().saturating_sub(1) * 2
}

pub(super) fn format_cell(value: &str, width: usize, align: Align) -> String {
	let value = fit_text(value, width, FitMode::Tail);
	match align {
		Align::Left => format!("{value:<width$}"),
		Align::Right => format!("{value:>width$}"),
	}
}

pub(super) fn fit_text(value: &str, width: usize, mode: FitMode) -> String {
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

pub(super) fn visible_len(value: &str) -> usize {
	value.chars().count()
}
