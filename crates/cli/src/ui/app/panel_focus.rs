use crate::ui::app::{
	App, FocusRegion, PanelNavigationState, ShellAction, scope_label, sync_contextual_view,
};
use crate::ui::render::view;
use crate::ui::shell::ShellEvent;
use crate::ui::{clipboard, explorer, panel};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum FocusCycle {
	Forward,
	Backward,
}

pub(in crate::ui) fn copy_panel_snapshot(app: &mut App) {
	let panel = explorer::active_panel(app);
	let snapshot = panel::panel_snapshot(&panel, view::current_panel_snapshot_width());
	let component = snapshot.component.as_str().to_string();
	let text = snapshot.to_text(crate::ui::app::view_mode(app).label(), &scope_label(app));
	let Some(tx) = app.runtime.event_tx.clone() else {
		crate::ui::app::set_status(app, "clipboard copy unavailable before event loop start");
		return;
	};
	match clipboard::copy_text_async(component.clone(), text, move |result| {
		let _ = tx.send(ShellEvent::Clipboard(result));
	}) {
		Ok(()) => {
			crate::ui::app::set_status(app, format!("copying {component} snapshot to clipboard"))
		}
		Err(error) => crate::ui::app::set_status(app, format!("clipboard copy failed: {error:#}")),
	}
}

pub(in crate::ui) fn cycle_focus_region(app: &mut App, direction: FocusCycle) {
	let usage_open = crate::ui::app::usage_lens(app).is_some();
	let current = crate::ui::app::focus_region(app);
	let next = match (current, usage_open, direction) {
		(FocusRegion::Navigator, true, FocusCycle::Forward) => FocusRegion::Panel,
		(FocusRegion::Panel, true, FocusCycle::Forward) => FocusRegion::UsageLens,
		(FocusRegion::UsageLens, true, FocusCycle::Forward) => FocusRegion::Navigator,
		(FocusRegion::Navigator, true, FocusCycle::Backward) => FocusRegion::UsageLens,
		(FocusRegion::UsageLens, true, FocusCycle::Backward) => FocusRegion::Panel,
		(FocusRegion::Panel, true, FocusCycle::Backward) => FocusRegion::Navigator,
		(FocusRegion::Navigator, false, _) => FocusRegion::Panel,
		(FocusRegion::Panel, false, _) => FocusRegion::Navigator,
		(FocusRegion::UsageLens, false, _) => FocusRegion::Navigator,
	};
	crate::ui::app::dispatch_shell(app, ShellAction::SetFocusRegion(next));
	match next {
		FocusRegion::Panel => {
			ensure_active_panel_selection(app);
			crate::ui::app::set_status(
				app,
				"panel focused; up/down moves within panel, Tab/Shift+Tab moves focus",
			);
		}
		FocusRegion::UsageLens => {
			crate::ui::app::set_status(
				app,
				"usage tree focused; Tab/Shift+Tab moves focus, Esc returns to navigator",
			);
			sync_contextual_view(app);
		}
		FocusRegion::Navigator => crate::ui::app::set_status(app, "navigator focused"),
	}
}

pub(in crate::ui) fn ensure_active_panel_selection(app: &mut App) {
	let panel = explorer::active_panel_nav(app);
	let count = panel.navigation_len;
	let component = panel.component;
	let current = &app.app_store.shell().panel_navigation;
	let selected = if count == 0 {
		None
	} else if current.component == Some(component) {
		current.selected.map(|idx| idx.min(count - 1)).or(Some(0))
	} else {
		Some(0)
	};
	let scroll = if current.component == Some(component) {
		current.scroll
	} else {
		0
	};
	let expanded = if current.component == Some(component) {
		current.expanded.clone()
	} else {
		explorer::active_panel_default_expanded(app)
	};
	crate::ui::app::dispatch_shell(
		app,
		ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(component),
			selected,
			scroll,
			expanded,
		}),
	);
}

pub(in crate::ui) fn move_panel_selection(app: &mut App, direction: i8) {
	let panel = explorer::active_panel_nav(app);
	let count = panel.navigation_len;
	let component = panel.component;
	if count == 0 {
		scroll_panel_lines(app, direction);
		crate::ui::app::set_status(app, "panel has no navigable item; scrolled content");
		return;
	}
	let current = &app.app_store.shell().panel_navigation;
	let selected_idx = if current.component == Some(component) {
		current.selected.unwrap_or(0).min(count - 1)
	} else {
		0
	};
	let selected = if direction > 0 {
		(selected_idx + 1).min(count - 1)
	} else {
		selected_idx.saturating_sub(1)
	};
	crate::ui::app::dispatch_shell(
		app,
		ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(component),
			selected: Some(selected),
			scroll: current.scroll,
			expanded: current.expanded.clone(),
		}),
	);
	crate::ui::app::set_status(app, format!("panel item {}/{}", selected + 1, count));
}

pub(in crate::ui) fn move_panel_to_edge(app: &mut App, home: bool) {
	let panel = explorer::active_panel_nav(app);
	let count = panel.navigation_len;
	let component = panel.component;
	let selected = if count == 0 {
		None
	} else if home {
		Some(0)
	} else {
		Some(count - 1)
	};
	let current = &app.app_store.shell().panel_navigation;
	crate::ui::app::dispatch_shell(
		app,
		ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(component),
			selected,
			scroll: if home { 0 } else { current.scroll },
			expanded: current.expanded.clone(),
		}),
	);
	if count == 0 {
		crate::ui::app::set_status(app, "panel has no navigable item");
	} else {
		crate::ui::app::set_status(
			app,
			format!("panel item {}/{}", selected.unwrap_or(0) + 1, count),
		);
	}
}

fn scroll_panel_lines(app: &mut App, direction: i8) {
	let next = if direction > 0 {
		app.app_store
			.shell()
			.panel_navigation
			.scroll
			.saturating_add(1)
	} else {
		app.app_store
			.shell()
			.panel_navigation
			.scroll
			.saturating_sub(1)
	};
	crate::ui::app::dispatch_shell(app, ShellAction::SetPanelScroll(next));
}

pub(in crate::ui) fn toggle_panel_tree_node(app: &mut App) {
	let Some(row) = selected_panel_tree_row(app) else {
		crate::ui::app::set_status(app, "panel item has no child node");
		return;
	};
	if !row.has_children {
		crate::ui::app::set_status(app, "panel item has no child node");
		return;
	}
	let mut expanded = app.app_store.shell().panel_navigation.expanded.clone();
	let opened = if expanded.remove(&row.key) {
		false
	} else {
		expanded.insert(row.key.clone());
		true
	};
	apply_panel_expanded(app, expanded, None);
	crate::ui::app::set_status(
		app,
		format!(
			"{} panel node {}",
			if opened { "opened" } else { "closed" },
			row.label
		),
	);
}

pub(in crate::ui) fn open_panel_tree_node(app: &mut App) {
	let Some(row) = selected_panel_tree_row(app) else {
		crate::ui::app::set_status(app, "panel item has no child node");
		return;
	};
	if !row.has_children {
		crate::ui::app::set_status(app, "panel item has no child node");
		return;
	}
	if row.expanded {
		crate::ui::app::set_status(app, format!("panel node already open: {}", row.label));
		return;
	}
	let mut expanded = app.app_store.shell().panel_navigation.expanded.clone();
	expanded.insert(row.key.clone());
	apply_panel_expanded(app, expanded, None);
	crate::ui::app::set_status(app, format!("opened panel node {}", row.label));
}

pub(in crate::ui) fn close_panel_tree_node(app: &mut App) {
	let Some(row) = selected_panel_tree_row(app) else {
		crate::ui::app::set_status(app, "panel focused; Tab/Shift+Tab moves focus");
		return;
	};
	if row.has_children && row.expanded {
		let mut expanded = app.app_store.shell().panel_navigation.expanded.clone();
		expanded.remove(&row.key);
		apply_panel_expanded(app, expanded, None);
		crate::ui::app::set_status(app, format!("closed panel node {}", row.label));
		return;
	}
	if row.depth == 0 {
		crate::ui::app::set_status(app, "panel focused; Tab/Shift+Tab moves focus");
		return;
	}
	let rows = explorer::active_panel_tree_rows(app);
	let selected = app
		.app_store
		.shell()
		.panel_navigation
		.selected
		.unwrap_or(0)
		.min(rows.len() - 1);
	let parent_depth = row.depth - 1;
	let parent = rows[..selected]
		.iter()
		.rposition(|candidate| candidate.depth == parent_depth);
	apply_panel_expanded(
		app,
		app.app_store.shell().panel_navigation.expanded.clone(),
		parent,
	);
	crate::ui::app::set_status(app, "panel parent selected");
}

fn selected_panel_tree_row(app: &App) -> Option<crate::ui::render::tree::TreeRowVm> {
	let rows = explorer::active_panel_tree_rows(app);
	let selected = app.app_store.shell().panel_navigation.selected?;
	rows.get(selected).cloned()
}

fn apply_panel_expanded(
	app: &mut App,
	expanded: std::collections::BTreeSet<String>,
	selected: Option<usize>,
) {
	let panel = explorer::active_panel_nav(app);
	let count = explorer::active_panel_tree_rows_with_expanded(app, &expanded).len();
	let selected = selected
		.or(app.app_store.shell().panel_navigation.selected)
		.map(|idx| {
			if count == 0 {
				0
			} else {
				idx.min(count.saturating_sub(1))
			}
		});
	crate::ui::app::dispatch_shell(
		app,
		ShellAction::SetPanelNavigation(PanelNavigationState {
			component: Some(panel.component),
			selected,
			scroll: app.app_store.shell().panel_navigation.scroll,
			expanded,
		}),
	);
}
