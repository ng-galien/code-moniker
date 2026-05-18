mod model;
mod render;

use ratatui::layout::Rect;

pub(in crate::ui) use model::{PanelRenderState, PanelVm, ReferenceGroupVm};
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
