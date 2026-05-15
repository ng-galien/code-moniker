use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::inspect::DefLocation;
use crate::lines::line_range;

use super::App;
use super::theme::{SourceTheme, THEME};

pub(super) fn source_snippet_lines(
	app: &App,
	loc: &DefLocation,
	context: u32,
) -> Vec<Line<'static>> {
	let theme = THEME.source;
	let file = &app.index.files[loc.file];
	let Some((start, end)) = app.index.def(loc).position else {
		return Vec::new();
	};
	let (start_line, end_line) = line_range(&file.source, start, end);
	let first = start_line.saturating_sub(context).max(1);
	let last = end_line.saturating_add(context);
	let width = last.to_string().len().max(4);
	file.source
		.lines()
		.enumerate()
		.filter_map(|(idx, line)| {
			let line_no = idx as u32 + 1;
			(first <= line_no && line_no <= last).then(|| {
				source_line(
					line_no,
					width,
					line,
					start_line <= line_no && line_no <= end_line,
					theme,
				)
			})
		})
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
