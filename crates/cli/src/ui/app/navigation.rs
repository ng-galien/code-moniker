use crate::ui::workspace_read::{self, ChangeDetail};
use code_moniker_workspace::snapshot::SymbolId;
type DefLocation = SymbolId;

use crate::ui::app::{
	ActiveFilter, App, FocusRegion, PanelPolicy, ShellAction, View, VisualizationMode,
	queue_usage_lens_refresh,
};
use crate::ui::events::UiMode;
use crate::ui::store::navigation::{
	NavigationAction, NavigationPane, NavigationScope, NavigationSelection, navigation_pane_view,
	navigation_primary_view,
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

pub(in crate::ui) fn selected(app: &App) -> Option<DefLocation> {
	selected_nav_row(app).and_then(|row| match &row.kind {
		NavNodeKind::Def(loc) => Some(loc.clone()),
		_ => None,
	})
}

pub(in crate::ui) fn primary_selected(app: &App) -> Option<DefLocation> {
	primary_selected_nav_row(app).and_then(|row| match &row.kind {
		NavNodeKind::Def(loc) => Some(loc.clone()),
		_ => None,
	})
}

pub(in crate::ui) fn selected_change_detail(app: &App) -> Option<ChangeDetail> {
	selected_nav_row(app).and_then(|row| match &row.kind {
		NavNodeKind::Change(id) => {
			workspace_read::change_detail(crate::ui::app::store(app), id.clone())
		}
		NavNodeKind::Def(loc) => {
			workspace_read::change_detail_for_symbol(crate::ui::app::store(app), loc)
		}
		_ => None,
	})
}

pub(in crate::ui) fn selected_nav_row(app: &App) -> Option<&NavRow> {
	navigation_pane_view(app.app_store.navigation(), active_navigation_pane(app))
		.and_then(|pane| pane.selected_row())
}

pub(in crate::ui) fn primary_selected_nav_row(app: &App) -> Option<&NavRow> {
	navigation_primary_view(app.app_store.navigation()).selected_row()
}

pub(in crate::ui) fn active_navigation_pane(app: &App) -> NavigationPane {
	if crate::ui::app::focus_region(app) == FocusRegion::UsageLens {
		NavigationPane::UsageLens
	} else {
		NavigationPane::Primary
	}
}

pub(in crate::ui) fn refresh_results(app: &mut App, reset_expansion: bool) {
	let visible_defs = matching_defs(app);
	let expand_symbols = visible_defs.len() <= 200;
	app.app_store
		.dispatch_navigation(NavigationAction::SetScope {
			scope: navigation_scope(app),
			visible_defs,
			reset_expansion,
			expand_symbols,
		});
}

pub(in crate::ui) fn matching_defs(app: &App) -> Vec<DefLocation> {
	match crate::ui::app::active_filter(app) {
		ActiveFilter::HeaderSearch(results) => results.matches.clone(),
		ActiveFilter::Change => workspace_read::changed_defs(crate::ui::app::store(app)),
		ActiveFilter::None => Vec::new(),
	}
}

pub(in crate::ui) fn navigation_scope(app: &App) -> NavigationScope {
	if matches!(crate::ui::app::active_filter(app), ActiveFilter::Change) {
		NavigationScope::Change
	} else if is_filtered(app) {
		NavigationScope::Filtered
	} else {
		NavigationScope::Explorer
	}
}

pub(in crate::ui) fn select_def(app: &mut App, loc: DefLocation) {
	apply_navigation(app, primary_tree_selection(NavigationSelection::Def(loc)));
}

pub(in crate::ui) fn select_first_change(app: &mut App) {
	app.app_store
		.dispatch_navigation(primary_tree_selection(NavigationSelection::FirstChange));
}

pub(in crate::ui) fn filter_label(app: &App) -> String {
	if matches!(crate::ui::app::mode(app), UiMode::HeaderSearch(_)) {
		let header = crate::ui::app::header_search(app);
		return super::header_search::header_search_label(
			&header.text,
			&header.langs,
			&header.kind_filters,
		);
	}
	let base = match crate::ui::app::active_filter(app) {
		ActiveFilter::None => "<all>".to_string(),
		ActiveFilter::HeaderSearch(results) => results.label(),
		ActiveFilter::Change => "changes".to_string(),
	};
	if let Some(focus) = crate::ui::app::usage_lens(app) {
		format!("{base} + usages:{}", focus.label)
	} else {
		base
	}
}

pub(in crate::ui) fn is_filtered(app: &App) -> bool {
	crate::ui::app::active_filter(app).filters_navigator()
}

pub(in crate::ui) fn has_clearable_scope(app: &App) -> bool {
	!matches!(crate::ui::app::active_filter(app), ActiveFilter::None)
}

pub(in crate::ui) fn contextual_view(app: &App) -> View {
	match crate::ui::app::view_mode(app) {
		VisualizationMode::Change => View::Change,
		VisualizationMode::Explorer | VisualizationMode::Search => {
			if matches!(crate::ui::app::view(app), View::Views | View::Notes) {
				return crate::ui::app::view(app);
			}
			if selected(app).is_some() {
				View::Tree
			} else if crate::ui::app::usage_lens(app).is_some()
				&& crate::ui::app::focus_region(app) == FocusRegion::UsageLens
			{
				View::Refs
			} else {
				View::Overview
			}
		}
	}
}

pub(in crate::ui) fn sync_contextual_view(app: &mut App) {
	if crate::ui::app::panel_policy(app) == PanelPolicy::Contextual {
		set_view(app, contextual_view(app), PanelPolicy::Contextual);
	}
}

pub(in crate::ui) fn set_view(app: &mut App, view: View, policy: PanelPolicy) {
	crate::ui::app::dispatch_shell(app, ShellAction::SetView { view, policy });
}

pub(in crate::ui) fn scope_label(app: &App) -> String {
	let base = match crate::ui::app::active_filter(app) {
		ActiveFilter::None => "all".to_string(),
		ActiveFilter::HeaderSearch(results) => results.label(),
		ActiveFilter::Change => workspace_read::change_overview(crate::ui::app::store(app)).scope,
	};
	if let Some(focus) = crate::ui::app::usage_lens(app) {
		format!("{base} + usages:{}", focus.label)
	} else {
		base
	}
}

pub(in crate::ui) fn toggle_selected_nav(app: &mut App) {
	let outcome = app.app_store.dispatch_navigation(focused_tree_action(
		crate::ui::app::focus_region(app),
		TreePaneAction::ToggleSelected,
	));
	match outcome.notice {
		TreePaneNotice::Opened(label) => crate::ui::app::set_status(app, format!("opened {label}")),
		TreePaneNotice::Closed(label) => crate::ui::app::set_status(app, format!("closed {label}")),
		TreePaneNotice::MovedToParent | TreePaneNotice::Noop => {}
	}
}

pub(in crate::ui) fn open_selected_nav(app: &mut App) {
	let outcome = app.app_store.dispatch_navigation(focused_tree_action(
		crate::ui::app::focus_region(app),
		TreePaneAction::OpenSelected,
	));
	if let TreePaneNotice::Opened(label) = outcome.notice {
		crate::ui::app::set_status(app, format!("opened {label}"));
	}
}

pub(in crate::ui) fn close_selected_nav(app: &mut App) -> bool {
	let outcome = app.app_store.dispatch_navigation(focused_tree_action(
		crate::ui::app::focus_region(app),
		TreePaneAction::CloseSelected,
	));
	match outcome.notice {
		TreePaneNotice::Closed(label) => {
			crate::ui::app::set_status(app, format!("closed {label}"));
			true
		}
		TreePaneNotice::MovedToParent => {
			sync_contextual_view(app);
			true
		}
		TreePaneNotice::Opened(_) => false,
		TreePaneNotice::Noop if crate::ui::app::focus_region(app) == FocusRegion::UsageLens => {
			crate::ui::app::dispatch_shell(
				app,
				ShellAction::SetFocusRegion(FocusRegion::Navigator),
			);
			sync_contextual_view(app);
			crate::ui::app::set_status(app, "navigator focused");
			true
		}
		TreePaneNotice::Noop => false,
	}
}

pub(in crate::ui) fn apply_navigation(app: &mut App, action: NavigationAction) {
	let outcome = app.app_store.dispatch_navigation(action);
	if outcome.primary_selection_changed && crate::ui::app::usage_lens(app).is_some() {
		queue_usage_lens_refresh(app);
	}
	if outcome.changed {
		sync_contextual_view(app);
	}
}
