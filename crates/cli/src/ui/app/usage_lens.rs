use code_moniker_workspace::snapshot::SymbolId;
type DefLocation = SymbolId;

use crate::ui::app::{App, ShellAction, VisualizationMode, primary_selected, sync_contextual_view};
use crate::ui::store::navigation::NavigationAction;
use crate::ui::workspace_read;

impl App {
	pub(in crate::ui) fn refresh_usage_lens_for_primary_selection(&mut self) {
		let Some(loc) = primary_selected(self) else {
			return;
		};
		let Some(focus) = workspace_read::usage_focus(crate::ui::app::store(self), loc) else {
			return;
		};
		let (label, refs_len, contexts_len) = self.set_usage_lens(focus, false);
		crate::ui::app::set_status(
			self,
			format!(
				"usage lens for {label}: {refs_len} reference(s), {contexts_len} navigable context(s)"
			),
		);
	}

	fn set_usage_lens(
		&mut self,
		focus: crate::ui::workspace_read::UsageFocus,
		move_focus: bool,
	) -> (String, usize, usize) {
		let label = focus.label.clone();
		let refs_len = focus.refs.len();
		let contexts_len = focus.contexts.len();
		let visible_defs = focus.contexts.clone();
		let expand_symbols = contexts_len <= 200;
		if move_focus {
			crate::ui::app::dispatch_shell(self, ShellAction::SetUsageLens(Some(focus)));
		} else {
			crate::ui::app::dispatch_shell(self, ShellAction::ReplaceUsageLens(focus));
		}
		self.app_store
			.dispatch_navigation(NavigationAction::SetUsageLens {
				visible_defs,
				reset_expansion: true,
				expand_symbols,
			});
		(label, refs_len, contexts_len)
	}

	pub(in crate::ui) fn focus_usages_of_selected(&mut self) {
		if crate::ui::app::view_mode(self) == VisualizationMode::Change {
			self.toggle_change_usages();
			return;
		}
		if crate::ui::app::usage_lens(self).is_some() {
			self.close_usage_lens();
			return;
		}
		let Some(loc) = primary_selected(self) else {
			crate::ui::app::set_status(self, "select a declaration before focusing usages");
			return;
		};
		self.focus_usages(loc);
	}

	pub(in crate::ui) fn focus_usages(&mut self, loc: DefLocation) {
		let Some(focus) = workspace_read::usage_focus(crate::ui::app::store(self), loc) else {
			crate::ui::app::set_status(self, "selected declaration has no usage information");
			return;
		};
		let (label, refs_len, contexts_len) = self.set_usage_lens(focus, true);
		sync_contextual_view(self);
		crate::ui::app::set_status(
			self,
			format!(
				"usage lens for {label}: {refs_len} reference(s), {contexts_len} navigable context(s)"
			),
		);
	}

	pub(in crate::ui) fn close_usage_lens(&mut self) {
		let label = crate::ui::app::usage_lens(self)
			.map(|focus| focus.label.clone())
			.unwrap_or_else(|| "usage lens".to_string());
		crate::ui::app::dispatch_shell(self, ShellAction::SetUsageLens(None));
		self.app_store
			.dispatch_navigation(NavigationAction::ClearUsageLens);
		sync_contextual_view(self);
		crate::ui::app::set_status(self, format!("closed usage lens for {label}"));
	}
}
