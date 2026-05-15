use ratatui::layout::Rect;

use super::{Effect, Route};
use crate::ui::component::ComponentId;
use crate::ui::events::Msg;

pub(in crate::ui) struct RenderContext<'a> {
	pub(in crate::ui) route: &'a Route,
}

pub(in crate::ui) struct ScreenContext<'a> {
	pub(in crate::ui) route: &'a Route,
}

pub(in crate::ui) trait Screen {
	fn title(&self) -> String;
	fn component(&self) -> ComponentId;

	fn render(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect, ctx: &RenderContext<'_>);

	fn handle_msg(&mut self, msg: Msg, ctx: &mut ScreenContext<'_>) -> anyhow::Result<Vec<Effect>>;
}
