// code-moniker: ignore-file[smell-god-type-local-metrics]
// TODO(smell): split App panel-focus helpers into scrolling, selection, clipboard, and detail-opening commands before enabling this guardrail here.
use crate::ui::app::{App, FocusRegion, PanelNavigationState, ShellAction};
use crate::ui::render::view;
use crate::ui::shell::ShellEvent;
use crate::ui::{clipboard, explorer, panel};

impl App {
	pub(in crate::ui) fn panel_scroll(&self) -> usize {
		self.app_store.shell().panel_navigation.scroll
	}

	pub(in crate::ui) fn panel_navigation(&self) -> &PanelNavigationState {
		&self.app_store.shell().panel_navigation
	}

	pub(in crate::ui) fn reset_panel_navigation(&mut self) {
		if self.app_store.shell().panel_navigation == PanelNavigationState::default() {
			return;
		}
		self.dispatch_shell(ShellAction::SetPanelNavigation(
			PanelNavigationState::default(),
		));
	}

	pub(in crate::ui) fn copy_panel_snapshot(&mut self) {
		let panel = explorer::active_panel(self);
		let snapshot = panel::panel_snapshot(&panel, view::current_panel_snapshot_width());
		let component = snapshot.component.as_str().to_string();
		let text = snapshot.to_text(self.view_mode().label(), &self.scope_label());
		let Some(tx) = self.event_tx.clone() else {
			self.set_status("clipboard copy unavailable before event loop start");
			return;
		};
		match clipboard::copy_text_async(component.clone(), text, move |result| {
			let _ = tx.send(ShellEvent::Clipboard(result));
		}) {
			Ok(()) => self.set_status(format!("copying {component} snapshot to clipboard")),
			Err(error) => self.set_status(format!("clipboard copy failed: {error:#}")),
		}
	}

	pub(in crate::ui) fn toggle_focus_region(&mut self) {
		let usage_open = self.usage_lens().is_some();
		let next = match (self.focus_region(), usage_open) {
			(FocusRegion::Navigator, true) => FocusRegion::UsageLens,
			(FocusRegion::Navigator, false) => FocusRegion::Panel,
			(FocusRegion::UsageLens, _) => FocusRegion::Panel,
			(FocusRegion::Panel, _) => FocusRegion::Navigator,
		};
		self.dispatch_shell(ShellAction::SetFocusRegion(next));
		match next {
			FocusRegion::Panel => {
				self.ensure_active_panel_selection();
				self.set_status("panel focused; up/down moves within panel, Tab moves focus");
			}
			FocusRegion::UsageLens => {
				self.set_status("usage tree focused; Tab moves to panel, Esc returns to navigator");
				self.sync_contextual_view();
			}
			FocusRegion::Navigator => self.set_status("navigator focused"),
		}
	}

	pub(in crate::ui) fn ensure_active_panel_selection(&mut self) {
		let panel = explorer::active_panel_nav(self);
		let count = panel.navigation_len;
		let component = panel.component;
		let selected = if count == 0 {
			None
		} else if self.panel_navigation().component == Some(component) {
			self.panel_navigation()
				.selected
				.map(|idx| idx.min(count - 1))
				.or(Some(0))
		} else {
			Some(0)
		};
		let scroll = if self.panel_navigation().component == Some(component) {
			self.panel_navigation().scroll
		} else {
			0
		};
		let expanded = if self.panel_navigation().component == Some(component) {
			self.panel_navigation().expanded.clone()
		} else {
			explorer::active_panel_default_expanded(self)
		};
		self.dispatch_shell(ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(component),
			selected,
			scroll,
			expanded,
		}));
	}

	pub(in crate::ui) fn move_panel_selection(&mut self, direction: i8) {
		let panel = explorer::active_panel_nav(self);
		let count = panel.navigation_len;
		let component = panel.component;
		if count == 0 {
			self.scroll_panel_lines(direction);
			self.set_status("panel has no navigable item; scrolled content");
			return;
		}
		let current = if self.panel_navigation().component == Some(component) {
			self.panel_navigation().selected.unwrap_or(0).min(count - 1)
		} else {
			0
		};
		let selected = if direction > 0 {
			(current + 1).min(count - 1)
		} else {
			current.saturating_sub(1)
		};
		self.dispatch_shell(ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(component),
			selected: Some(selected),
			scroll: self.panel_scroll(),
			expanded: self.panel_navigation().expanded.clone(),
		}));
		self.set_status(format!("panel item {}/{}", selected + 1, count));
	}

	pub(in crate::ui) fn move_panel_to_edge(&mut self, home: bool) {
		let panel = explorer::active_panel_nav(self);
		let count = panel.navigation_len;
		let component = panel.component;
		let selected = if count == 0 {
			None
		} else if home {
			Some(0)
		} else {
			Some(count - 1)
		};
		self.dispatch_shell(ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(component),
			selected,
			scroll: if home { 0 } else { self.panel_scroll() },
			expanded: self.panel_navigation().expanded.clone(),
		}));
		if count == 0 {
			self.set_status("panel has no navigable item");
		} else {
			self.set_status(format!(
				"panel item {}/{}",
				selected.unwrap_or(0) + 1,
				count
			));
		}
	}

	pub(in crate::ui) fn scroll_panel_lines(&mut self, direction: i8) {
		let next = if direction > 0 {
			self.panel_scroll().saturating_add(1)
		} else {
			self.panel_scroll().saturating_sub(1)
		};
		self.dispatch_shell(ShellAction::SetPanelScroll(next));
	}

	pub(in crate::ui) fn toggle_panel_tree_node(&mut self) {
		let Some(row) = self.selected_panel_tree_row() else {
			self.set_status("panel item has no child node");
			return;
		};
		if !row.has_children {
			self.set_status("panel item has no child node");
			return;
		}
		let mut expanded = self.panel_navigation().expanded.clone();
		let opened = if expanded.remove(&row.key) {
			false
		} else {
			expanded.insert(row.key.clone());
			true
		};
		self.apply_panel_expanded(expanded, None);
		self.set_status(format!(
			"{} panel node {}",
			if opened { "opened" } else { "closed" },
			row.label
		));
	}

	pub(in crate::ui) fn open_panel_tree_node(&mut self) {
		let Some(row) = self.selected_panel_tree_row() else {
			self.set_status("panel item has no child node");
			return;
		};
		if !row.has_children {
			self.set_status("panel item has no child node");
			return;
		}
		if row.expanded {
			self.set_status(format!("panel node already open: {}", row.label));
			return;
		}
		let mut expanded = self.panel_navigation().expanded.clone();
		expanded.insert(row.key.clone());
		self.apply_panel_expanded(expanded, None);
		self.set_status(format!("opened panel node {}", row.label));
	}

	pub(in crate::ui) fn close_panel_tree_node(&mut self) {
		let Some(row) = self.selected_panel_tree_row() else {
			self.set_status("panel focused; Tab moves focus");
			return;
		};
		if row.has_children && row.expanded {
			let mut expanded = self.panel_navigation().expanded.clone();
			expanded.remove(&row.key);
			self.apply_panel_expanded(expanded, None);
			self.set_status(format!("closed panel node {}", row.label));
			return;
		}
		if row.depth == 0 {
			self.set_status("panel focused; Tab moves focus");
			return;
		}
		let rows = explorer::active_panel_tree_rows(self);
		let selected = self
			.panel_navigation()
			.selected
			.unwrap_or(0)
			.min(rows.len() - 1);
		let parent_depth = row.depth - 1;
		let parent = rows[..selected]
			.iter()
			.rposition(|candidate| candidate.depth == parent_depth);
		self.apply_panel_expanded(self.panel_navigation().expanded.clone(), parent);
		self.set_status("panel parent selected");
	}

	fn selected_panel_tree_row(&self) -> Option<crate::ui::render::tree::TreeRowVm> {
		let rows = explorer::active_panel_tree_rows(self);
		let selected = self.panel_navigation().selected?;
		rows.get(selected).cloned()
	}

	fn apply_panel_expanded(
		&mut self,
		expanded: std::collections::BTreeSet<String>,
		selected: Option<usize>,
	) {
		let panel = explorer::active_panel_nav(self);
		let count = explorer::active_panel_tree_rows_with_expanded(self, &expanded).len();
		let selected = selected.or(self.panel_navigation().selected).map(|idx| {
			if count == 0 {
				0
			} else {
				idx.min(count.saturating_sub(1))
			}
		});
		self.dispatch_shell(ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(panel.component),
			selected,
			scroll: self.panel_scroll(),
			expanded,
		}));
	}
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use super::*;
	use crate::session::SessionOptions;
	use crate::ui::app::{AppAction, View};
	use crate::ui::events::Msg;
	use crate::ui::render::component::ComponentId;
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
			"src/app.ts",
			"export function run() { return MissingService.create(); }\n",
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
	fn close_node_keeps_focus_inside_panel() {
		let mut app = fixture_app();
		app.set_view(View::Unresolved, crate::ui::app::PanelPolicy::Manual);
		app.dispatch_shell(ShellAction::SetFocusRegion(FocusRegion::Panel));
		app.ensure_active_panel_selection();

		app.update(AppAction::Ui(Msg::CloseNode));

		assert_eq!(app.focus_region(), FocusRegion::Panel);
		assert_eq!(app.status(), "closed panel node ts/");
		assert_eq!(
			app.panel_navigation().component,
			Some(ComponentId::PanelUnresolved)
		);
		assert_eq!(app.panel_navigation().selected, Some(0));
		assert_eq!(explorer::active_panel_tree_rows(&app).len(), 1);
	}

	#[test]
	fn open_node_expands_panel_tree_without_touching_navigator_focus() {
		let mut app = fixture_app();
		app.set_view(View::Unresolved, crate::ui::app::PanelPolicy::Manual);
		app.dispatch_shell(ShellAction::SetFocusRegion(FocusRegion::Panel));
		app.ensure_active_panel_selection();
		app.update(AppAction::Ui(Msg::CloseNode));

		app.update(AppAction::Ui(Msg::OpenNode));

		assert_eq!(app.focus_region(), FocusRegion::Panel);
		assert_eq!(app.status(), "opened panel node ts/");
		assert!(explorer::active_panel_tree_rows(&app).len() > 1);
	}
}
