use ratatui::layout::Rect;

use super::Route;
use crate::ui::component::ComponentId;

pub(in crate::ui) struct RenderContext<'a> {
	pub(in crate::ui) route: &'a Route,
}

pub(in crate::ui) trait Screen {
	fn title(&self) -> String;
	fn component(&self) -> ComponentId;

	fn render(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &RenderContext<'_>);
}
