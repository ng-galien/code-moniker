use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme::THEME;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(in crate::ui) enum ComponentId {
	Header,
	SearchInput,
	Navigator,
	NavigatorUsages,
	Status,
	PanelOverview,
	PanelOutline,
	PanelRefs,
	PanelUsages,
	PanelCheck,
	PanelChange,
	SourceSnippet,
}

impl ComponentId {
	pub(in crate::ui) fn as_str(self) -> &'static str {
		match self {
			Self::Header => "ui.header",
			Self::SearchInput => "ui.search.input",
			Self::Navigator => "ui.navigator",
			Self::NavigatorUsages => "ui.navigator.usages",
			Self::Status => "ui.status",
			Self::PanelOverview => "ui.panel.overview",
			Self::PanelOutline => "ui.panel.outline",
			Self::PanelRefs => "ui.panel.refs",
			Self::PanelUsages => "ui.panel.usages",
			Self::PanelCheck => "ui.panel.check",
			Self::PanelChange => "ui.panel.change",
			Self::SourceSnippet => "ui.source.snippet",
		}
	}
}

pub(in crate::ui) fn marker(id: ComponentId) -> Span<'static> {
	raw_marker(id.as_str())
}

pub(in crate::ui) fn raw_marker(id: &'static str) -> Span<'static> {
	Span::styled(
		format!("[{id}]"),
		Style::default().fg(THEME.component_marker),
	)
}

pub(in crate::ui) fn block_title(label: impl Into<String>, id: ComponentId) -> Line<'static> {
	let label = label.into();
	Line::from(vec![
		Span::raw(label.trim().to_string()),
		Span::raw(" "),
		marker(id),
	])
}

pub(in crate::ui) fn focused_block_title(
	label: impl Into<String>,
	id: ComponentId,
	focused: bool,
) -> Line<'static> {
	if !focused {
		return block_title(label, id);
	}
	let label = label.into();
	let style = Style::default()
		.fg(THEME.focus.title)
		.add_modifier(Modifier::BOLD);
	Line::from(vec![
		Span::styled(label.trim().to_string(), style),
		Span::raw(" "),
		Span::styled(format!("[{}]", id.as_str()), style),
	])
}
