use ratatui::layout::Rect;

use crate::ui::component::ComponentId;
use crate::ui::contracts::{RenderContext, Screen};
use crate::ui::{App, view};

pub(in crate::ui) struct ExplorerScreen<'a> {
	app: &'a mut App,
}

impl<'a> ExplorerScreen<'a> {
	pub(in crate::ui) fn new(app: &'a mut App) -> Self {
		Self { app }
	}
}

impl Screen for ExplorerScreen<'_> {
	fn title(&self) -> String {
		"Explorer".to_string()
	}

	fn component(&self) -> ComponentId {
		ComponentId::PanelOverview
	}

	fn render(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &RenderContext<'_>) {
		let _ = ctx.route;
		view::render_shell(frame, area, self.app);
	}
}
