use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::text::{FitMode, fit_text};
use super::theme::THEME;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct TreeRowVm {
	pub(in crate::ui) key: String,
	pub(in crate::ui) depth: usize,
	pub(in crate::ui) has_children: bool,
	pub(in crate::ui) expanded: bool,
	pub(in crate::ui) label: String,
	pub(in crate::ui) meta: Option<String>,
	pub(in crate::ui) detail: Option<String>,
}

impl TreeRowVm {
	pub(in crate::ui) fn new(
		key: impl Into<String>,
		depth: usize,
		label: impl Into<String>,
	) -> Self {
		Self {
			key: key.into(),
			depth,
			has_children: false,
			expanded: false,
			label: label.into(),
			meta: None,
			detail: None,
		}
	}

	pub(in crate::ui) fn branch(mut self, expanded: bool) -> Self {
		self.has_children = true;
		self.expanded = expanded;
		self
	}

	pub(in crate::ui) fn meta(mut self, meta: impl Into<String>) -> Self {
		self.meta = Some(meta.into());
		self
	}

	pub(in crate::ui) fn detail(mut self, detail: impl Into<String>) -> Self {
		self.detail = Some(detail.into());
		self
	}
}

pub(in crate::ui) fn prefix_spans(
	depth: usize,
	has_children: bool,
	expanded: bool,
	selected: Option<bool>,
) -> Vec<Span<'static>> {
	let mut spans = Vec::new();
	if let Some(selected) = selected {
		let marker = if selected { ">" } else { " " };
		spans.push(Span::styled(marker, Style::default().fg(THEME.nav.marker)));
		spans.push(Span::raw(" "));
	}
	spans.push(Span::raw("  ".repeat(depth)));
	let twisty = if has_children {
		if expanded { "▾" } else { "▸" }
	} else {
		" "
	};
	spans.push(Span::styled(twisty, Style::default().fg(THEME.nav.twisty)));
	spans.push(Span::raw(" "));
	spans
}

pub(in crate::ui) fn panel_tree_row(row: &TreeRowVm, width: usize) -> Line<'static> {
	let mut spans = prefix_spans(row.depth, row.has_children, row.expanded, None);
	spans.push(Span::styled(
		row.label.clone(),
		Style::default()
			.fg(THEME.panel.value)
			.add_modifier(if row.has_children {
				Modifier::BOLD
			} else {
				Modifier::empty()
			}),
	));
	if let Some(meta) = &row.meta {
		spans.push(Span::styled(
			format!("  {meta}"),
			Style::default().fg(THEME.panel.muted),
		));
	}
	if let Some(detail) = &row.detail {
		spans.push(Span::styled(
			format!("  {}", fit_text(detail, width / 2, FitMode::Tail)),
			Style::default().fg(THEME.panel.muted),
		));
	}
	Line::from(spans)
}
