mod model;
mod render;

use ratatui::layout::Rect;

pub(in crate::ui) use model::{PanelVm, ReferenceGroupVm};
pub(in crate::ui) use render::PanelSnapshot;
#[cfg(test)]
pub(in crate::ui) use render::source_snippet_lines;

pub(super) fn render_panel(frame: &mut ratatui::Frame<'_>, area: Rect, panel: &PanelVm) {
	render::render_panel_vm(frame, area, panel);
}

pub(super) fn panel_snapshot(panel: &PanelVm, width: usize) -> PanelSnapshot {
	render::snapshot(panel, width)
}
