use code_moniker_workspace::snapshot::SymbolId;
type DefLocation = SymbolId;

use crate::ui::app::{App, ShellAction, VisualizationMode};
use crate::ui::store::navigation::NavigationAction;

impl App {
	pub(in crate::ui) fn focus_usages(&mut self, loc: DefLocation) {
		let Some(focus) = self.store().usage_focus(loc) else {
			self.set_status("selected declaration has no usage information");
			return;
		};
		let (label, refs_len, contexts_len) = self.set_usage_lens(focus, true);
		self.sync_contextual_view();
		self.set_status(format!(
			"usage lens for {label}: {refs_len} reference(s), {contexts_len} navigable context(s)"
		));
	}

	pub(in crate::ui) fn refresh_usage_lens_for_primary_selection(&mut self) {
		let Some(loc) = self.primary_selected() else {
			return;
		};
		let Some(focus) = self.store().usage_focus(loc) else {
			return;
		};
		let (label, refs_len, contexts_len) = self.set_usage_lens(focus, false);
		self.set_status(format!(
			"usage lens for {label}: {refs_len} reference(s), {contexts_len} navigable context(s)"
		));
	}

	fn set_usage_lens(
		&mut self,
		focus: crate::ui::workspace_state::UsageFocus,
		move_focus: bool,
	) -> (String, usize, usize) {
		let label = focus.label.clone();
		let refs_len = focus.refs.len();
		let contexts_len = focus.contexts.len();
		let visible_defs = focus.contexts.clone();
		let expand_symbols = contexts_len <= 200;
		if move_focus {
			self.dispatch_shell(ShellAction::SetUsageLens(Some(focus)));
		} else {
			self.dispatch_shell(ShellAction::ReplaceUsageLens(focus));
		}
		self.dispatch_navigation(NavigationAction::SetUsageLens {
			visible_defs,
			reset_expansion: true,
			expand_symbols,
		});
		(label, refs_len, contexts_len)
	}

	pub(in crate::ui) fn focus_usages_of_selected(&mut self) {
		if self.view_mode() == VisualizationMode::Change {
			self.toggle_change_usages();
			return;
		}
		if self.usage_lens().is_some() {
			self.close_usage_lens();
			return;
		}
		let Some(loc) = self.primary_selected() else {
			self.set_status("select a declaration before focusing usages");
			return;
		};
		self.focus_usages(loc);
	}

	pub(in crate::ui) fn close_usage_lens(&mut self) {
		let label = self
			.usage_lens()
			.map(|focus| focus.label.clone())
			.unwrap_or_else(|| "usage lens".to_string());
		self.dispatch_shell(ShellAction::SetUsageLens(None));
		self.dispatch_navigation(NavigationAction::ClearUsageLens);
		self.sync_contextual_view();
		self.set_status(format!("closed usage lens for {label}"));
	}
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use crate::session::SessionOptions;
	use crate::ui::app::App;
	use crate::ui::workspace_state::WorkspaceState;

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
		let store = WorkspaceState::load(&SessionOptions {
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
	fn focus_usages_without_primary_selection_only_reports_status() {
		let mut app = fixture_app();

		app.focus_usages_of_selected();

		assert!(app.usage_lens().is_none());
		assert_eq!(app.status(), "select a declaration before focusing usages");
	}
}
