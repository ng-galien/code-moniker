use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::theme::THEME;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) enum ComponentId {
	Header,
	Navigator,
	Status,
	PanelOverview,
	PanelOutline,
	PanelRefs,
	PanelUsages,
	PanelCheck,
	SourceSnippet,
}

impl ComponentId {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::Header => "ui.header",
			Self::Navigator => "ui.navigator",
			Self::Status => "ui.status",
			Self::PanelOverview => "ui.panel.overview",
			Self::PanelOutline => "ui.panel.outline",
			Self::PanelRefs => "ui.panel.refs",
			Self::PanelUsages => "ui.panel.usages",
			Self::PanelCheck => "ui.panel.check",
			Self::SourceSnippet => "ui.source.snippet",
		}
	}
}

pub(super) fn marker(id: ComponentId) -> Span<'static> {
	Span::styled(
		format!("[{}]", id.as_str()),
		Style::default().fg(THEME.component_marker),
	)
}

pub(super) fn block_title(label: impl Into<String>, id: ComponentId) -> Line<'static> {
	let label = label.into();
	Line::from(vec![
		Span::raw(label.trim().to_string()),
		Span::raw(" "),
		marker(id),
	])
}
