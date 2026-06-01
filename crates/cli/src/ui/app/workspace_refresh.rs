use std::time::Instant;

use crate::ui::app::{
	ActiveFilter, App, ShellAction, queue_task, refresh_results, select_first_change,
	sync_contextual_view,
};
use crate::ui::async_task::TaskSpec;
use crate::ui::live::StoreEvent;
use crate::ui::perf;
use crate::ui::store::navigation::{NavigationAction, navigation_primary_view};
use crate::ui::store::navigation_tree::{build_change_navigator, build_navigator};
use crate::ui::workspace_read;

#[derive(Clone, Copy)]
enum StoreChangeKind {
	FileCatalog,
	Reloaded,
	GitOverlay,
}

pub(in crate::ui) fn handle_store_event(app: &mut App, event: StoreEvent) {
	if queue_store_task(app, event) {
		return;
	}
	handle_store_event_sync(app, event);
}

fn queue_store_task(app: &mut App, event: StoreEvent) -> bool {
	let task = match event {
		StoreEvent::GitOverlay => linkage_or_index_task(app),
		StoreEvent::FullIndex => TaskSpec::load_symbol_index(crate::ui::app::store_options(app)),
	};
	queue_task(app, task)
}

fn linkage_or_index_task(app: &App) -> TaskSpec {
	let Some(snapshot) = crate::ui::app::store(app).queries().snapshot_arc() else {
		return TaskSpec::load_symbol_index(crate::ui::app::store_options(app));
	};
	if snapshot.index.symbols.is_empty() {
		return TaskSpec::load_symbol_index(crate::ui::app::store_options(app));
	}
	TaskSpec::resolve_linkage(
		crate::ui::app::store_options(app),
		app.workspace.cache().clone(),
		snapshot,
	)
}

pub(in crate::ui) fn handle_store_event_sync(app: &mut App, event: StoreEvent) {
	match event {
		StoreEvent::GitOverlay => {
			if crate::ui::workspace_read::refresh_workspace(crate::ui::app::store_mut(app)).is_ok()
			{
				crate::ui::app::publish_workspace_snapshot(app);
			}
			apply_refreshed_change_store(app, "git overlay refreshed".to_string());
		}
		StoreEvent::FullIndex => {
			match crate::ui::workspace_read::refresh_workspace(crate::ui::app::store_mut(app)) {
				Ok(()) => {
					crate::ui::app::publish_workspace_snapshot(app);
					apply_reloaded_store(app, "store reloaded after filesystem change".to_string());
				}
				Err(error) => {
					crate::ui::app::set_status(app, format!("store reload failed: {error:#}"));
				}
			}
		}
	}
}

pub(in crate::ui) fn apply_file_catalog_store(app: &mut App, status: String) {
	refresh_ui_after_store_change(app, StoreChangeKind::FileCatalog, status);
}

pub(in crate::ui) fn apply_reloaded_store(app: &mut App, status: String) {
	refresh_ui_after_store_change(app, StoreChangeKind::Reloaded, status);
}

fn apply_refreshed_change_store(app: &mut App, status: String) {
	refresh_ui_after_store_change(app, StoreChangeKind::GitOverlay, status);
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
	let explorer = build_navigator(crate::ui::app::store(app));
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
