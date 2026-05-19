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
				self.set_status(
					"panel focused; up/down moves within panel, Esc returns to navigator",
				);
			}
			FocusRegion::UsageLens => {
				self.set_status("usage tree focused; Tab moves to panel, Esc returns to navigator");
				self.sync_contextual_view();
			}
			FocusRegion::Navigator => self.set_status("navigator focused"),
		}
	}

	pub(in crate::ui) fn ensure_active_panel_selection(&mut self) {
		let panel = explorer::active_panel(self);
		let count = panel.navigation_len();
		let component = panel.component();
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
		self.dispatch_shell(ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(component),
			selected,
			scroll,
		}));
	}

	pub(in crate::ui) fn move_panel_selection(&mut self, direction: i8) {
		let panel = explorer::active_panel(self);
		let count = panel.navigation_len();
		let component = panel.component();
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
		}));
		self.set_status(format!("panel item {}/{}", selected + 1, count));
	}

	pub(in crate::ui) fn move_panel_to_edge(&mut self, home: bool) {
		let panel = explorer::active_panel(self);
		let count = panel.navigation_len();
		let component = panel.component();
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
}
