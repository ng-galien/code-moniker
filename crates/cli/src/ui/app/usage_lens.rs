use crate::workspace::{DefLocation, IndexStore};

use crate::ui::App;
use crate::ui::app::{ChangePanelMode, PanelPolicy, ShellAction, View, VisualizationMode};
use crate::ui::store::navigation::NavigationAction;

impl App {
	pub(in crate::ui) fn focus_usages(&mut self, loc: DefLocation) {
		let focus = self.store().usage_focus(loc);
		let label = focus.label.clone();
		let refs_len = focus.refs.len();
		let contexts_len = focus.contexts.len();
		let visible_defs = focus.contexts.clone();
		self.dispatch_shell(ShellAction::SetUsageLens(Some(focus)));
		self.dispatch_navigation(NavigationAction::SetUsageLens {
			visible_defs,
			reset_expansion: true,
			expand_symbols: contexts_len <= 200,
		});
		self.sync_contextual_view();
		self.set_status(format!(
			"usage lens for {label}: {} reference(s), {} navigable context(s)",
			refs_len, contexts_len
		));
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
