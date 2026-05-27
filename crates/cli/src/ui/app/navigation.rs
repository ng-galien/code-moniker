// code-moniker: ignore-file[smell-god-type-local-metrics]
// TODO(smell): split App navigation helpers into focused-tree actions, route selection, and panel policy transitions before enabling this guardrail here.
use crate::ui::workspace_read::{ChangeDetail, WorkspaceRead};
use code_moniker_workspace::snapshot::SymbolId;
type DefLocation = SymbolId;

use crate::ui::app::{
	ActiveFilter, App, FocusRegion, PanelPolicy, ShellAction, View, VisualizationMode,
};
use crate::ui::events::UiMode;
use crate::ui::store::navigation::{
	NavigationAction, NavigationPane, NavigationScope, NavigationSelection,
};
use crate::ui::store::navigation_tree::{NavNodeKind, NavRow};
use crate::ui::store::tree_pane_action::{TreePaneAction, TreePaneNotice};

fn focused_tree_action(focus: FocusRegion, action: TreePaneAction) -> NavigationAction {
	let pane = if focus == FocusRegion::UsageLens {
		NavigationPane::UsageLens
	} else {
		NavigationPane::Primary
	};
	NavigationAction::Pane { pane, action }
}

fn primary_tree_selection(target: NavigationSelection) -> NavigationAction {
	NavigationAction::Select {
		pane: NavigationPane::Primary,
		target,
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct NavigationDispatchOutcome {
	pub(in crate::ui) changed: bool,
	pub(in crate::ui) selection_changed: bool,
	pub(in crate::ui) primary_selection_changed: bool,
	pub(in crate::ui) notice: TreePaneNotice,
}

impl App {
	pub(in crate::ui) fn selected(&self) -> Option<DefLocation> {
		self.selected_nav_row().and_then(|row| match &row.kind {
			NavNodeKind::Def(loc) => Some(loc.clone()),
			_ => None,
		})
	}

	pub(in crate::ui) fn primary_selected(&self) -> Option<DefLocation> {
		self.primary_selected_nav_row()
			.and_then(|row| match &row.kind {
				NavNodeKind::Def(loc) => Some(loc.clone()),
				_ => None,
			})
	}

	pub(in crate::ui) fn selected_change_detail(&self) -> Option<ChangeDetail> {
		self.selected_nav_row().and_then(|row| match &row.kind {
			NavNodeKind::Change(id) => self.store().change_detail(id.clone()),
			NavNodeKind::Def(loc) => self.store().change_detail_for_symbol(loc),
			_ => None,
		})
	}

	pub(in crate::ui) fn selected_nav_row(&self) -> Option<&NavRow> {
		self.app_store
			.navigation()
			.pane_view(self.active_navigation_pane())
			.and_then(|pane| pane.selected_row())
	}

	pub(in crate::ui) fn primary_selected_nav_row(&self) -> Option<&NavRow> {
		self.app_store.navigation().primary_view().selected_row()
	}

	pub(in crate::ui) fn active_navigation_pane(&self) -> NavigationPane {
		if self.focus_region() == FocusRegion::UsageLens {
			NavigationPane::UsageLens
		} else {
			NavigationPane::Primary
		}
	}

	pub(in crate::ui) fn dispatch_navigation(
		&mut self,
		action: NavigationAction,
	) -> NavigationDispatchOutcome {
		let before = self.selected_nav_row().map(|row| row.key.clone());
		let before_primary = self.primary_selected_nav_row().map(|row| row.key.clone());
		let (changed, effects) = {
			let transition = self.app_store.dispatch_navigation(action);
			(transition.changed, transition.take_effects())
		};
		self.apply_effects(effects);
		let selection_changed =
			changed && before != self.selected_nav_row().map(|row| row.key.clone());
		if selection_changed {
			self.reset_panel_navigation();
		}
		NavigationDispatchOutcome {
			changed,
			selection_changed,
			primary_selection_changed: changed
				&& before_primary != self.primary_selected_nav_row().map(|row| row.key.clone()),
			notice: self.app_store.navigation().last_notice().clone(),
		}
	}

	pub(in crate::ui) fn refresh_results(&mut self, reset_expansion: bool) {
		let visible_defs = self.matching_defs();
		let expand_symbols = visible_defs.len() <= 200;
		self.dispatch_navigation(NavigationAction::SetScope {
			scope: self.navigation_scope(),
			visible_defs,
			reset_expansion,
			expand_symbols,
		});
	}

	pub(in crate::ui) fn matching_defs(&self) -> Vec<DefLocation> {
		match self.active_filter() {
			ActiveFilter::HeaderSearch(results) => results.matches.clone(),
			ActiveFilter::Change => self.store().changed_defs(),
			ActiveFilter::None => self.store().all_navigable_defs(),
		}
	}

	pub(in crate::ui) fn navigation_scope(&self) -> NavigationScope {
		if matches!(self.active_filter(), ActiveFilter::Change) {
			NavigationScope::Change
		} else if self.is_filtered() {
			NavigationScope::Filtered
		} else {
			NavigationScope::Explorer
		}
	}

	pub(in crate::ui) fn select_def(&mut self, loc: DefLocation) {
		self.apply_navigation(primary_tree_selection(NavigationSelection::Def(loc)));
	}

	pub(in crate::ui) fn select_first_change(&mut self) {
		self.dispatch_navigation(primary_tree_selection(NavigationSelection::FirstChange));
	}

	pub(in crate::ui) fn filter_label(&self) -> String {
		if matches!(self.mode(), UiMode::HeaderSearch(_)) {
			let header = self.header_search();
			return super::header_search::header_search_label(
				&header.text,
				&header.langs,
				&header.kind_filters,
			);
		}
		let base = match self.active_filter() {
			ActiveFilter::None => "<all>".to_string(),
			ActiveFilter::HeaderSearch(results) => results.label(),
			ActiveFilter::Change => "changes".to_string(),
		};
		if let Some(focus) = self.usage_lens() {
			format!("{base} + usages:{}", focus.label)
		} else {
			base
		}
	}

	pub(in crate::ui) fn is_filtered(&self) -> bool {
		self.active_filter().filters_navigator()
	}

	pub(in crate::ui) fn has_clearable_scope(&self) -> bool {
		!matches!(self.active_filter(), ActiveFilter::None)
	}

	pub(in crate::ui) fn contextual_view(&self) -> View {
		match self.view_mode() {
			VisualizationMode::Change => View::Change,
			VisualizationMode::Explorer | VisualizationMode::Search => {
				if self.selected().is_some() {
					View::Tree
				} else if self.usage_lens().is_some()
					&& self.focus_region() == FocusRegion::UsageLens
				{
					View::Refs
				} else {
					View::Overview
				}
			}
		}
	}

	pub(in crate::ui) fn sync_contextual_view(&mut self) {
		if self.panel_policy() == PanelPolicy::Contextual {
			self.set_view(self.contextual_view(), PanelPolicy::Contextual);
		}
	}

	pub(in crate::ui) fn set_view(&mut self, view: View, policy: PanelPolicy) {
		self.dispatch_shell(ShellAction::SetView { view, policy });
	}

	pub(in crate::ui) fn scope_label(&self) -> String {
		let base = match self.active_filter() {
			ActiveFilter::None => "all".to_string(),
			ActiveFilter::HeaderSearch(results) => results.label(),
			ActiveFilter::Change => self.store().change_overview().scope,
		};
		if let Some(focus) = self.usage_lens() {
			format!("{base} + usages:{}", focus.label)
		} else {
			base
		}
	}

	pub(in crate::ui) fn toggle_selected_nav(&mut self) {
		let outcome = self.dispatch_navigation(focused_tree_action(
			self.focus_region(),
			TreePaneAction::ToggleSelected,
		));
		match outcome.notice {
			TreePaneNotice::Opened(label) => self.set_status(format!("opened {label}")),
			TreePaneNotice::Closed(label) => self.set_status(format!("closed {label}")),
			TreePaneNotice::MovedToParent | TreePaneNotice::Noop => {}
		}
	}

	pub(in crate::ui) fn open_selected_nav(&mut self) {
		let outcome = self.dispatch_navigation(focused_tree_action(
			self.focus_region(),
			TreePaneAction::OpenSelected,
		));
		if let TreePaneNotice::Opened(label) = outcome.notice {
			self.set_status(format!("opened {label}"));
		}
	}

	pub(in crate::ui) fn close_selected_nav(&mut self) -> bool {
		let outcome = self.dispatch_navigation(focused_tree_action(
			self.focus_region(),
			TreePaneAction::CloseSelected,
		));
		match outcome.notice {
			TreePaneNotice::Closed(label) => {
				self.set_status(format!("closed {label}"));
				true
			}
			TreePaneNotice::MovedToParent => {
				self.sync_contextual_view();
				true
			}
			TreePaneNotice::Opened(_) => false,
			TreePaneNotice::Noop if self.focus_region() == FocusRegion::UsageLens => {
				self.dispatch_shell(ShellAction::SetFocusRegion(FocusRegion::Navigator));
				self.sync_contextual_view();
				self.set_status("navigator focused");
				true
			}
			TreePaneNotice::Noop => false,
		}
	}

	pub(in crate::ui) fn apply_navigation(&mut self, action: NavigationAction) {
		let outcome = self.dispatch_navigation(action);
		if outcome.primary_selection_changed && self.usage_lens().is_some() {
			self.refresh_usage_lens_for_primary_selection();
		}
		if outcome.changed {
			self.sync_contextual_view();
		}
	}
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use super::*;
	use crate::session::SessionOptions;
	use crate::ui::app::{App, AppAction, PanelNavigationState, ShellAction};
	use crate::ui::events::Msg;
	use crate::ui::render::component::ComponentId;
	use crate::ui::store::tree_pane_action::TreePaneAction;
	use crate::ui::workspace_read::load_local_workspace;

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
			"export class AlphaService {}\nexport class BetaService {}\nexport function useAlpha() { return new AlphaService(); }\nexport function useBeta() { return new BetaService(); }\n",
		);
		let opts = SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		};
		let (store, cache) = load_local_workspace(&opts).unwrap();
		App::new(
			store,
			cache,
			opts,
			"default".to_string(),
			tmp.path().join("rules.toml"),
			None,
		)
	}

	fn def_named(app: &App, name: &str) -> DefLocation {
		app.store()
			.all_navigable_defs()
			.into_iter()
			.find(|loc| app.store().symbol_summary(loc).name == name)
			.unwrap_or_else(|| panic!("missing def {name}"))
	}

	#[test]
	fn navigation_selection_change_resets_panel_navigation() {
		let mut app = fixture_app();
		app.toggle_selected_nav();
		assert!(app.navigation().primary_view().rows.len() > 1);
		app.dispatch_shell(ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(ComponentId::PanelOverview),
			selected: Some(2),
			scroll: 8,
			expanded: Default::default(),
		}));

		app.apply_navigation(NavigationAction::Pane {
			pane: NavigationPane::Primary,
			action: TreePaneAction::MoveDown,
		});

		assert_eq!(app.panel_navigation(), &PanelNavigationState::default());
	}

	#[test]
	fn toggling_selected_navigation_node_reports_open_or_close_status() {
		let mut app = fixture_app();

		app.toggle_selected_nav();

		assert!(
			app.status().starts_with("opened ") || app.status().starts_with("closed "),
			"status was {:?}",
			app.status()
		);
	}

	#[test]
	fn primary_navigation_updates_open_usage_lens() {
		let mut app = fixture_app();
		let alpha = def_named(&app, "AlphaService");
		let beta = def_named(&app, "BetaService");
		let visible_defs = app.store().all_navigable_defs();
		app.dispatch_navigation(NavigationAction::SetScope {
			scope: NavigationScope::Filtered,
			visible_defs,
			reset_expansion: true,
			expand_symbols: true,
		});
		app.apply_navigation(NavigationAction::Select {
			pane: NavigationPane::Primary,
			target: NavigationSelection::Def(alpha.clone()),
		});
		app.focus_usages(alpha.clone());
		assert_eq!(
			app.usage_lens().map(|focus| focus.label.as_str()),
			Some("AlphaService")
		);
		app.dispatch_shell(ShellAction::SetFocusRegion(FocusRegion::Navigator));

		app.apply_navigation(NavigationAction::Select {
			pane: NavigationPane::Primary,
			target: NavigationSelection::Def(beta),
		});

		let focus = app.usage_lens().expect("usage lens remains open");
		assert_eq!(app.focus_region(), FocusRegion::Navigator);
		assert_eq!(focus.label, "BetaService");
		assert_eq!(
			app.navigation().usage_view().map(|pane| pane.rows.len()),
			Some(focus.contexts.len())
		);
	}

	#[test]
	fn select_def_refreshes_open_usage_lens() {
		let mut app = fixture_app();
		let alpha = def_named(&app, "AlphaService");
		let beta = def_named(&app, "BetaService");
		let visible_defs = app.store().all_navigable_defs();
		app.dispatch_navigation(NavigationAction::SetScope {
			scope: NavigationScope::Filtered,
			visible_defs,
			reset_expansion: true,
			expand_symbols: true,
		});
		app.select_def(alpha.clone());
		app.focus_usages(alpha);
		app.dispatch_shell(ShellAction::SetFocusRegion(FocusRegion::Navigator));

		app.select_def(beta);

		assert_eq!(
			app.usage_lens().map(|focus| focus.label.as_str()),
			Some("BetaService")
		);
		assert_eq!(app.focus_region(), FocusRegion::Navigator);
	}

	#[test]
	fn primary_navigation_to_non_definition_preserves_open_usage_lens() {
		let mut app = fixture_app();
		let alpha = def_named(&app, "AlphaService");
		let visible_defs = app.store().all_navigable_defs();
		app.dispatch_navigation(NavigationAction::SetScope {
			scope: NavigationScope::Filtered,
			visible_defs,
			reset_expansion: true,
			expand_symbols: true,
		});
		app.focus_usages(alpha.clone());
		assert!(app.usage_lens().is_some());
		app.dispatch_shell(ShellAction::SetFocusRegion(FocusRegion::Navigator));
		app.apply_navigation(NavigationAction::Select {
			pane: NavigationPane::Primary,
			target: NavigationSelection::Def(alpha.clone()),
		});
		assert_eq!(app.primary_selected(), Some(alpha));

		app.apply_navigation(NavigationAction::Pane {
			pane: NavigationPane::Primary,
			action: TreePaneAction::CloseSelected,
		});

		assert!(app.primary_selected().is_none());
		assert_eq!(
			app.usage_lens().map(|focus| focus.label.as_str()),
			Some("AlphaService")
		);
		assert!(app.navigation().usage_view().is_some());
		assert_eq!(app.focus_region(), FocusRegion::Navigator);
	}

	#[test]
	fn ui_move_down_refreshes_open_usage_lens_through_runtime_path() {
		let mut app = fixture_app();
		let alpha = def_named(&app, "AlphaService");
		let visible_defs = app.store().all_navigable_defs();
		app.dispatch_navigation(NavigationAction::SetScope {
			scope: NavigationScope::Filtered,
			visible_defs,
			reset_expansion: true,
			expand_symbols: true,
		});
		app.focus_usages(alpha.clone());
		app.dispatch_shell(ShellAction::SetFocusRegion(FocusRegion::Navigator));
		app.apply_navigation(NavigationAction::Select {
			pane: NavigationPane::Primary,
			target: NavigationSelection::Def(alpha),
		});

		app.update(AppAction::Ui(Msg::MoveDown));

		assert_ne!(
			app.usage_lens().map(|focus| focus.label.as_str()),
			Some("AlphaService")
		);
	}

	#[test]
	fn usage_lens_alone_is_not_a_clearable_scope() {
		let mut app = fixture_app();
		let alpha = def_named(&app, "AlphaService");
		app.focus_usages(alpha);
		app.dispatch_shell(ShellAction::SetFocusRegion(FocusRegion::Navigator));

		assert!(!app.has_clearable_scope());
	}
}
