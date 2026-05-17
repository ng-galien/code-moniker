use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::workspace::DefLocation;

use super::App;
use super::theme::{SourceTheme, THEME};
use crate::workspace::IndexStore;

pub(super) fn source_snippet_lines(
	app: &App,
	loc: &DefLocation,
	context: u32,
) -> Vec<Line<'static>> {
	let theme = THEME.source;
	let snippet = app.store().source_snippet(loc, context);
	let width = snippet
		.iter()
		.map(|line| line.number.to_string().len())
		.max()
		.unwrap_or(4)
		.max(4);
	snippet
		.into_iter()
		.map(|line| source_line(line.number, width, &line.text, line.active, theme))
		.collect()
}

fn source_line(
	line_no: u32,
	width: usize,
	text: &str,
	active: bool,
	theme: SourceTheme,
) -> Line<'static> {
	let line_bg = if active {
		theme.active_bg
	} else {
		theme.context_bg
	};
	let number_style = if active {
		Style::default()
			.fg(theme.active_number_fg)
			.bg(line_bg)
			.add_modifier(Modifier::BOLD)
	} else {
		Style::default().fg(theme.context_number_fg).bg(line_bg)
	};
	let code_style = if active {
		Style::default().fg(theme.active_fg).bg(line_bg)
	} else {
		Style::default().fg(theme.context_fg).bg(line_bg)
	};
	let indent_style = if active {
		Style::default().bg(theme.active_indent_bg)
	} else {
		Style::default().bg(theme.context_indent_bg)
	};
	let gutter = if active { " │ " } else { " ┆ " };
	let (indent, body) = split_leading_ws(text);
	let mut spans = vec![
		Span::styled(format!("{line_no:>width$}"), number_style),
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
