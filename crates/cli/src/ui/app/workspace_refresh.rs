use std::time::Instant;

use crate::ui::app::{
	ActiveFilter, App, ShellAction, queue_task, refresh_results, select_first_change,
	sync_contextual_view,
};
use crate::ui::async_task::TaskSpec;
use crate::ui::perf;
use crate::ui::store::navigation::{NavigationAction, navigation_primary_view};
use crate::ui::store::navigation_tree::{build_change_navigator, build_navigator};
use crate::ui::workspace_read;
use code_moniker_workspace::live::{WorkspaceLiveEvent, WorkspaceLiveRefreshPlan};

#[derive(Clone, Copy)]
enum StoreChangeKind {
	FileCatalog,
	Reloaded,
	Notes,
}

pub(in crate::ui) fn handle_store_event(app: &mut App, event: WorkspaceLiveEvent) {
	handle_store_event_sync(app, event);
}

pub(in crate::ui) fn handle_store_event_sync(app: &mut App, event: WorkspaceLiveEvent) {
	let plan = WorkspaceLiveRefreshPlan::from_event(event);
	if should_queue_workspace_live_plan(&plan) {
		queue_workspace_live_plan(app, plan);
		return;
	}
	if plan.includes_notes() {
		refresh_workspace_notes(app);
	}
}

fn should_queue_workspace_live_plan(plan: &WorkspaceLiveRefreshPlan) -> bool {
	plan.requires_rescan() || !plan.source_paths().is_empty() || plan.includes_git_base()
}

fn refresh_workspace_notes(app: &mut App) {
	match crate::ui::app::reload_notes(app) {
		Ok(()) => {
			refresh_ui_after_store_change(
				app,
				StoreChangeKind::Notes,
				"notes refreshed".to_string(),
			);
		}
		Err(error) => {
			crate::ui::app::set_status(app, format!("notes reload failed: {error:#}"));
		}
	}
}

fn queue_workspace_live_plan(app: &mut App, plan: WorkspaceLiveRefreshPlan) {
	let Some(snapshot) = crate::ui::app::store(app).queries().snapshot_arc() else {
		queue_workspace_rescan(app);
		return;
	};
	let task = TaskSpec::refresh_workspace_live_plan(
		crate::ui::app::store_options(app),
		app.workspace.cache().clone(),
		snapshot,
		plan,
	);
	if queue_task(app, task) {
		crate::ui::app::set_status(app, "live workspace refresh queued in background");
	}
}

fn queue_workspace_rescan(app: &mut App) {
	if queue_task(
		app,
		TaskSpec::load_symbol_index(crate::ui::app::store_options(app)),
	) {
		crate::ui::app::set_status(app, "workspace rescan queued in background");
	}
}

pub(in crate::ui) fn apply_file_catalog_store(app: &mut App, status: String) {
	refresh_ui_after_store_change(app, StoreChangeKind::FileCatalog, status);
}

pub(in crate::ui) fn apply_reloaded_store(app: &mut App, status: String) {
	refresh_ui_after_store_change(app, StoreChangeKind::Reloaded, status);
}

fn refresh_ui_after_store_change(app: &mut App, kind: StoreChangeKind, status: String) {
	let total_started = Instant::now();
	if matches!(
		kind,
		StoreChangeKind::FileCatalog | StoreChangeKind::Reloaded
	) {
		let started = Instant::now();
		app.runtime.watch_roots_update = Some(crate::ui::app::store_watch_roots(app));
		app.refresh_header_search_options();
		perf::record(
			"store_refresh.watch_search",
			started.elapsed(),
			status.as_str(),
		);
	}
	let reset = matches!(crate::ui::app::active_filter(app), ActiveFilter::Change)
		&& navigation_primary_view(app.app_store.navigation())
			.rows
			.is_empty();
	if matches!(kind, StoreChangeKind::Reloaded) {
		let started = Instant::now();
		refresh_active_filter_after_store_reload(app);
		perf::record(
			"store_refresh.active_filter",
			started.elapsed(),
			status.as_str(),
		);
	}
	let started = Instant::now();
	let explorer = build_navigator(
		crate::ui::app::store(app),
		&crate::ui::app::store_options(app).paths,
	);
	perf::record(
		"store_refresh.build_navigator",
		started.elapsed(),
		status.as_str(),
	);
	let started = Instant::now();
	let change = build_change_navigator(crate::ui::app::store(app));
	perf::record(
		"store_refresh.build_change_navigator",
		started.elapsed(),
		status.as_str(),
	);
	let started = Instant::now();
	app.app_store
		.dispatch_navigation(NavigationAction::ReplaceModels { explorer, change });
	perf::record(
		"store_refresh.replace_models",
		started.elapsed(),
		status.as_str(),
	);
	let started = Instant::now();
	refresh_results(app, matches!(kind, StoreChangeKind::FileCatalog) || reset);
	perf::record(
		"store_refresh.refresh_results",
		started.elapsed(),
		status.as_str(),
	);
	if reset && !matches!(kind, StoreChangeKind::FileCatalog) {
		let started = Instant::now();
		select_first_change(app);
		perf::record(
			"store_refresh.select_first_change",
			started.elapsed(),
			status.as_str(),
		);
	}
	let started = Instant::now();
	sync_contextual_view(app);
	perf::record(
		"store_refresh.sync_contextual_view",
		started.elapsed(),
		status.as_str(),
	);
	crate::ui::app::set_status(app, status);
	perf::record(
		"store_refresh.total",
		total_started.elapsed(),
		crate::ui::app::status(app),
	);
}

fn refresh_active_filter_after_store_reload(app: &mut App) {
	let active_filter = match crate::ui::app::active_filter(app) {
		ActiveFilter::HeaderSearch(results) => ActiveFilter::HeaderSearch(
			app.header_search_results(&results.text, &results.langs, &results.kind_filters),
		),
		ActiveFilter::None => ActiveFilter::None,
		ActiveFilter::Change => ActiveFilter::Change,
	};
	crate::ui::app::dispatch_shell(app, ShellAction::ReplaceActiveFilter(active_filter));
	refresh_usage_lens_after_store_reload(app);
}

fn refresh_usage_lens_after_store_reload(app: &mut App) {
	let Some(focus) = crate::ui::app::usage_lens(app).cloned() else {
		return;
	};
	let Some(focus) = workspace_read::usage_focus_for_target(
		crate::ui::app::store(app),
		focus.target,
		focus.label,
	) else {
		return;
	};
	let visible_defs = focus.contexts.clone();
	let expand_symbols = visible_defs.len() <= 200;
	crate::ui::app::dispatch_shell(app, ShellAction::SetUsageLens(Some(focus)));
	app.app_store
		.dispatch_navigation(NavigationAction::SetUsageLens {
			visible_defs,
			reset_expansion: false,
			expand_symbols,
		});
}
