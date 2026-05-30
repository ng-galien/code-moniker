use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::perf;
use crate::session::{SessionOptions, StoreWatchRoot};
use crate::ui::events::UiMode;
use crate::ui::shell::ShellEvent;
use crate::ui::store::navigation::NavigationState;
use crate::ui::store::navigation_tree::{build_change_navigator, build_navigator};
use crate::ui::workspace_read::{
	LocalWorkspaceFacade, UsageFocus, new_local_workspace, workspace_check_context,
};
use code_moniker_workspace::source::LocalResourceCache;

mod action;
mod change_mode;
mod effect;
mod header_search;
mod navigation;
mod panel_focus;
mod runtime;
mod state;
mod store;
mod usage_lens;
mod workspace_refresh;
mod workspace_session;

pub(in crate::ui) use action::{AppAction, ShellAction};
pub(in crate::ui) use effect::Effect;
pub(in crate::ui) use header_search::{
	HeaderKindFilter, HeaderSearchState, display_filter, header_search_label, kind_filter_summary,
	lang_filter_summary,
};
pub(in crate::ui) use navigation::{
	apply_navigation, close_selected_nav, filter_label, has_clearable_scope, is_filtered,
	open_selected_nav, primary_selected, refresh_results, scope_label, select_def,
	select_first_change, selected, selected_change_detail, set_view, sync_contextual_view,
	toggle_selected_nav,
};
pub(in crate::ui) use panel_focus::{
	close_panel_tree_node, copy_panel_snapshot, ensure_active_panel_selection,
	move_panel_selection, move_panel_to_edge, open_panel_tree_node, toggle_focus_region,
	toggle_panel_tree_node,
};
pub(in crate::ui) use runtime::{
	dispatch_and_apply, handle_key, queue_startup_load, queue_task, queue_usage_lens_refresh,
	set_event_sender, take_watch_roots_update, update,
};
pub(in crate::ui) use state::{
	ActiveFilter, ChangePanelMode, CheckState, FocusRegion, PanelNavigationState, PanelPolicy,
	TaskCompletion, View, VisualizationMode,
};
pub(in crate::ui) use store::AppStore;
pub(in crate::ui) use workspace_refresh::{
	apply_file_catalog_store, apply_reloaded_store, handle_store_event, handle_store_event_sync,
};
use workspace_session::WorkspaceSession;

pub(in crate::ui) struct App {
	pub(in crate::ui) app_store: AppStore,
	pub(in crate::ui) workspace: WorkspaceSession,
	pub(in crate::ui) check: AppCheckConfig,
	pub(in crate::ui) runtime: AppRuntime,
}

pub(in crate::ui) struct AppCheckConfig {
	scheme: String,
	rules: PathBuf,
	profile: Option<String>,
}

pub(in crate::ui) struct AppRuntime {
	event_tx: Option<Sender<ShellEvent>>,
	startup_load_pending: bool,
	watch_roots_update: Option<Vec<StoreWatchRoot>>,
	usage_lens_generation: u64,
}

pub(in crate::ui) fn boot_app(
	opts: SessionOptions,
	scheme: String,
	rules: PathBuf,
	profile: Option<String>,
) -> App {
	let (store, cache) = new_local_workspace(&opts);
	let mut app = new_app(store, cache, opts, scheme, rules, profile);
	app.runtime.startup_load_pending = true;
	set_status(&mut app, "loading index...");
	app
}

pub(in crate::ui) fn status(app: &App) -> &str {
	app.app_store.status()
}

pub(in crate::ui) fn set_status(app: &mut App, status: impl Into<String>) {
	dispatch_shell(app, ShellAction::SetStatus(status.into()));
}

pub(in crate::ui) fn append_status(app: &mut App, status: impl AsRef<str>) {
	dispatch_shell(app, ShellAction::AppendStatus(status.as_ref().to_string()));
}

pub(in crate::ui) fn check_state(app: &App) -> &CheckState {
	app.app_store.check_state()
}

pub(in crate::ui) fn set_check_state(app: &mut App, state: CheckState) {
	dispatch_shell(app, ShellAction::SetCheckState(state));
}

pub(in crate::ui) fn dispatch_shell(app: &mut App, action: ShellAction) {
	let refresh_search_options = matches!(
		action,
		ShellAction::SetHeaderSearchFilters { .. } | ShellAction::ClearFilter { .. }
	);
	dispatch_and_apply(app, &AppAction::Shell(action));
	if refresh_search_options {
		app.refresh_header_search_options();
	}
}

pub(in crate::ui) fn view(app: &App) -> View {
	app.app_store.shell().view
}

pub(in crate::ui) fn view_mode(app: &App) -> VisualizationMode {
	app.app_store.shell().view_mode
}

pub(in crate::ui) fn panel_policy(app: &App) -> PanelPolicy {
	app.app_store.shell().panel_policy
}

pub(in crate::ui) fn change_panel(app: &App) -> ChangePanelMode {
	app.app_store.shell().change_panel
}

pub(in crate::ui) fn mode(app: &App) -> UiMode {
	app.app_store.shell().mode
}

pub(in crate::ui) fn focus_region(app: &App) -> FocusRegion {
	app.app_store.shell().focus_region
}

pub(in crate::ui) fn usage_lens(app: &App) -> Option<&UsageFocus> {
	app.app_store.shell().usage_lens.as_ref()
}

pub(in crate::ui) fn active_filter(app: &App) -> &ActiveFilter {
	&app.app_store.shell().active_filter
}

pub(in crate::ui) fn header_search(app: &App) -> &HeaderSearchState {
	&app.app_store.shell().header_search
}

pub(in crate::ui) fn navigation(app: &App) -> &NavigationState {
	app.app_store.navigation()
}

pub(in crate::ui) fn store(app: &App) -> &LocalWorkspaceFacade {
	app.workspace.store()
}

pub(in crate::ui) fn store_mut(app: &mut App) -> &mut LocalWorkspaceFacade {
	app.workspace.store_mut()
}

pub(in crate::ui) fn store_options(app: &App) -> SessionOptions {
	app.workspace.options().clone()
}

pub(in crate::ui) fn store_root_label(app: &App) -> String {
	crate::session::root_label_for_options(app.workspace.options())
}

pub(in crate::ui) fn store_watch_roots(app: &App) -> Vec<StoreWatchRoot> {
	crate::session::watch_roots_for_options(app.workspace.options())
}

pub(in crate::ui) fn store_check_context(
	app: &App,
) -> anyhow::Result<crate::ui::workspace_read::WorkspaceCheckContext> {
	workspace_check_context(store(app), app.workspace.cache())
}

pub(in crate::ui) fn shared_workspace_index(
	app: &App,
) -> crate::workspace_index::SharedWorkspaceIndex {
	app.workspace.shared_index()
}

pub(in crate::ui) fn publish_workspace_snapshot(app: &App) {
	app.workspace.publish_current_snapshot();
}

pub(in crate::ui) fn replace_store(
	app: &mut App,
	store: LocalWorkspaceFacade,
	cache: LocalResourceCache,
	options: SessionOptions,
) {
	app.workspace.replace(store, cache, options);
}

pub(in crate::ui) fn app_rules_path(app: &App) -> &std::path::Path {
	&app.check.rules
}

pub(in crate::ui) fn app_profile_name(app: &App) -> Option<&str> {
	app.check.profile.as_deref()
}

pub(in crate::ui) fn new_app(
	store: LocalWorkspaceFacade,
	cache: LocalResourceCache,
	options: SessionOptions,
	scheme: String,
	rules: PathBuf,
	profile: Option<String>,
) -> App {
	let started = Instant::now();
	let navigator = build_navigator(&store);
	perf::record("app.new.build_navigator", started.elapsed(), "");
	let started = Instant::now();
	let change_navigator = build_change_navigator(&store);
	perf::record("app.new.build_change_navigator", started.elapsed(), "");
	let started = Instant::now();
	let mut app_store = AppStore::new();
	app_store.set_navigation(NavigationState::new(navigator, change_navigator));
	let mut app = App {
		app_store,
		workspace: WorkspaceSession::new(store, cache, options),
		check: AppCheckConfig {
			scheme,
			rules,
			profile,
		},
		runtime: AppRuntime {
			event_tx: None,
			startup_load_pending: false,
			watch_roots_update: None,
			usage_lens_generation: 0,
		},
	};
	app.refresh_header_search_options();
	set_status(
		&mut app,
		"Enter opens nodes, Esc/left closes, PgUp/PgDn scroll panel, s focuses search, x resets filters, d changes, u usages, y copies panel, c checks, q quits",
	);
	refresh_results(&mut app, false);
	perf::record("app.new.finish", started.elapsed(), status(&app));
	app
}
