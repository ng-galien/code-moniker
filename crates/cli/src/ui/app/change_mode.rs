use crate::workspace::IndexStore;

use crate::ui::app::{App, ChangePanelMode, PanelPolicy, ShellAction, View, VisualizationMode};

impl App {
	pub(in crate::ui) fn toggle_change_mode(&mut self) {
		if self.view_mode() == VisualizationMode::Change {
			self.clear_filter();
			return;
		}
		self.dispatch_shell(ShellAction::EnterChangeMode);
		self.refresh_results(true);
		self.select_first_change();
		self.sync_contextual_view();
		let changes = self.store().change_overview();
		self.set_status(format!(
			"changes: {} declaration(s) across {} file(s)",
			changes.change_count, changes.file_count
		));
	}

	pub(in crate::ui) fn toggle_change_usages(&mut self) {
		let Some(change) = self.selected_change_detail() else {
			self.set_status("select a changed declaration before toggling blast radius");
			return;
		};
		let name = change.summary.name;
		let next_panel = match self.change_panel() {
			ChangePanelMode::Diff => ChangePanelMode::Usages,
			ChangePanelMode::Usages => ChangePanelMode::Diff,
		};
		self.dispatch_shell(ShellAction::SetChangePanel(next_panel));
		self.set_view(View::Change, PanelPolicy::Contextual);
		self.set_status(match next_panel {
			ChangePanelMode::Diff => format!("change diff details for {name}"),
			ChangePanelMode::Usages => format!("change blast radius for {name}"),
		});
	}
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use crate::ui::app::{ActiveFilter, App, ChangePanelMode, VisualizationMode};
	use crate::workspace::{SessionOptions, WorkspaceStore};

	fn write(root: &Path, rel: &str, body: &str) {
		let path = root.join(rel);
		if let Some(parent) = path.parent() {
			std::fs::create_dir_all(parent).unwrap();
		}
		std::fs::write(path, body).unwrap();
	}

	fn fixture_app() -> App {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"src/services.ts",
			"export class AlphaService {}\n",
		);
		let store = WorkspaceStore::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		App::new(
			store,
			"default".to_string(),
			tmp.path().join("rules.toml"),
			None,
		)
	}

	#[test]
	fn toggle_change_mode_enters_change_scope() {
		let mut app = fixture_app();

		app.toggle_change_mode();

		assert_eq!(app.view_mode(), VisualizationMode::Change);
		assert!(matches!(app.active_filter(), ActiveFilter::Change));
		assert_eq!(app.change_panel(), ChangePanelMode::Diff);
		assert!(app.status().starts_with("changes: "));
	}

	#[test]
	fn toggle_change_mode_clears_change_scope_when_already_active() {
		let mut app = fixture_app();
		app.toggle_change_mode();

		app.toggle_change_mode();

		assert_eq!(app.view_mode(), VisualizationMode::Explorer);
		assert!(matches!(app.active_filter(), ActiveFilter::None));
		assert_eq!(app.change_panel(), ChangePanelMode::Diff);
	}
}
