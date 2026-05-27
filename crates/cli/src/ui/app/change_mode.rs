use crate::ui::app::{
	App, ChangePanelMode, PanelPolicy, ShellAction, View, VisualizationMode, refresh_results,
	select_first_change, selected_change_detail, set_view, sync_contextual_view,
};
use crate::ui::workspace_read;

impl App {
	pub(in crate::ui) fn toggle_change_mode(&mut self) {
		if crate::ui::app::view_mode(self) == VisualizationMode::Change {
			self.clear_filter();
			return;
		}
		crate::ui::app::dispatch_shell(self, ShellAction::EnterChangeMode);
		refresh_results(self, true);
		select_first_change(self);
		sync_contextual_view(self);
		let changes = workspace_read::change_overview(crate::ui::app::store(self));
		crate::ui::app::set_status(
			self,
			format!(
				"changes: {} declaration(s) across {} file(s)",
				changes.change_count, changes.file_count
			),
		);
	}

	pub(in crate::ui) fn toggle_change_usages(&mut self) {
		let Some(change) = selected_change_detail(self) else {
			crate::ui::app::set_status(
				self,
				"select a changed declaration before toggling blast radius",
			);
			return;
		};
		let name = change.summary.name;
		let next_panel = match crate::ui::app::change_panel(self) {
			ChangePanelMode::Diff => ChangePanelMode::Usages,
			ChangePanelMode::Usages => ChangePanelMode::Diff,
		};
		crate::ui::app::dispatch_shell(self, ShellAction::SetChangePanel(next_panel));
		set_view(self, View::Change, PanelPolicy::Contextual);
		crate::ui::app::set_status(
			self,
			match next_panel {
				ChangePanelMode::Diff => format!("change diff details for {name}"),
				ChangePanelMode::Usages => format!("change blast radius for {name}"),
			},
		);
	}
}
