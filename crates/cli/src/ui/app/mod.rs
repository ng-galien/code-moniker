// code-moniker: ignore-file[smell-feature-envy-local, smell-god-type-local-metrics, smell-harmonious-method-size]
// TODO(smell): split App shell construction from runtime dispatch, workspace refresh, navigation, and panel orchestration before enabling these guardrails here.
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

pub(in crate::ui) use action::{AppAction, ShellAction};
pub(in crate::ui) use effect::Effect;
pub(in crate::ui) use header_search::{
	HeaderKindFilter, HeaderSearchState, display_filter, header_search_label, kind_filter_summary,
	lang_filter_summary,
};
pub(in crate::ui) use state::{
	ActiveFilter, ChangePanelMode, CheckState, FocusRegion, PanelNavigationState, PanelPolicy,
	TaskCompletion, View, VisualizationMode,
};
pub(in crate::ui) use store::AppStore;

pub(in crate::ui) struct App {
	app_store: AppStore,
	scheme: String,
	rules: PathBuf,
	profile: Option<String>,
	event_tx: Option<Sender<ShellEvent>>,
	startup_load_pending: bool,
	watch_roots_update: Option<Vec<StoreWatchRoot>>,
}

impl App {
	pub(in crate::ui) fn new(
		store: LocalWorkspaceFacade,
		cache: LocalResourceCache,
		options: SessionOptions,
		scheme: String,
		rules: PathBuf,
		profile: Option<String>,
	) -> Self {
		let started = Instant::now();
		let navigator = build_navigator(&store);
		perf::record("app.new.build_navigator", started.elapsed(), "");
		let started = Instant::now();
		let change_navigator = build_change_navigator(&store);
		perf::record("app.new.build_change_navigator", started.elapsed(), "");
		let started = Instant::now();
		let mut app_store = AppStore::from_workspace(store, cache, options);
		app_store.set_navigation(NavigationState::new(navigator, change_navigator));
		let mut app = Self {
			app_store,
			scheme,
			rules,
			profile,
			event_tx: None,
			startup_load_pending: false,
			watch_roots_update: None,
		};
		app.refresh_header_search_options();
		app.set_status(
			"Enter opens nodes, Esc/left closes, PgUp/PgDn scroll panel, s focuses search, x resets filters, d changes, u usages, y copies panel, c checks, q quits",
		);
		app.refresh_results(false);
		perf::record("app.new.finish", started.elapsed(), app.status());
		app
	}

	pub(in crate::ui) fn boot(
		opts: SessionOptions,
		scheme: String,
		rules: PathBuf,
		profile: Option<String>,
	) -> Self {
		let (store, cache) = new_local_workspace(&opts);
		let mut app = Self::new(store, cache, opts, scheme, rules, profile);
		app.startup_load_pending = true;
		app.set_status("loading index...");
		app
	}

	pub(in crate::ui) fn status(&self) -> &str {
		self.app_store.status()
	}

	pub(in crate::ui) fn set_status(&mut self, status: impl Into<String>) {
		self.dispatch_shell(ShellAction::SetStatus(status.into()));
	}

	pub(in crate::ui) fn append_status(&mut self, status: impl AsRef<str>) {
		self.dispatch_shell(ShellAction::AppendStatus(status.as_ref().to_string()));
	}

	pub(in crate::ui) fn check_state(&self) -> &CheckState {
		self.app_store.check_state()
	}

	pub(in crate::ui) fn set_check_state(&mut self, state: CheckState) {
		self.dispatch_shell(ShellAction::SetCheckState(state));
	}

	pub(in crate::ui) fn dispatch_shell(&mut self, action: ShellAction) {
		let refresh_search_options = matches!(
			action,
			ShellAction::SetHeaderSearchFilters { .. } | ShellAction::ClearFilter { .. }
		);
		self.dispatch_and_apply(&AppAction::Shell(action));
		if refresh_search_options {
			self.refresh_header_search_options();
		}
	}

	pub(in crate::ui) fn view(&self) -> View {
		self.app_store.shell().view
	}

	pub(in crate::ui) fn view_mode(&self) -> VisualizationMode {
		self.app_store.shell().view_mode
	}

	pub(in crate::ui) fn panel_policy(&self) -> PanelPolicy {
		self.app_store.shell().panel_policy
	}

	pub(in crate::ui) fn change_panel(&self) -> ChangePanelMode {
		self.app_store.shell().change_panel
	}

	pub(in crate::ui) fn mode(&self) -> UiMode {
		self.app_store.shell().mode
	}

	pub(in crate::ui) fn focus_region(&self) -> FocusRegion {
		self.app_store.shell().focus_region
	}

	pub(in crate::ui) fn usage_lens(&self) -> Option<&UsageFocus> {
		self.app_store.shell().usage_lens.as_ref()
	}

	pub(in crate::ui) fn active_filter(&self) -> &ActiveFilter {
		&self.app_store.shell().active_filter
	}

	pub(in crate::ui) fn header_search(&self) -> &HeaderSearchState {
		&self.app_store.shell().header_search
	}

	pub(in crate::ui) fn navigation(&self) -> &NavigationState {
		self.app_store.navigation()
	}

	pub(in crate::ui) fn rules_path(&self) -> &std::path::Path {
		&self.rules
	}

	pub(in crate::ui) fn profile_name(&self) -> Option<&str> {
		self.profile.as_deref()
	}

	pub(in crate::ui) fn store(&self) -> &LocalWorkspaceFacade {
		self.app_store.workspace()
	}

	pub(in crate::ui) fn store_mut(&mut self) -> &mut LocalWorkspaceFacade {
		self.app_store.workspace_mut()
	}

	pub(in crate::ui) fn store_options(&self) -> SessionOptions {
		self.app_store.workspace_options().clone()
	}

	pub(in crate::ui) fn store_root_label(&self) -> String {
		match self.app_store.workspace_options().paths.as_slice() {
			[] => "<empty>".to_string(),
			[path] => path.display().to_string(),
			paths => paths
				.iter()
				.map(|path| path.display().to_string())
				.collect::<Vec<_>>()
				.join(", "),
		}
	}

	pub(in crate::ui) fn store_watch_roots(&self) -> Vec<StoreWatchRoot> {
		crate::session::watch_roots_for_options(self.app_store.workspace_options())
	}

	pub(in crate::ui) fn store_check_context(
		&self,
	) -> anyhow::Result<crate::ui::workspace_read::WorkspaceCheckContext> {
		workspace_check_context(self.store(), self.app_store.workspace_cache())
	}

	pub(in crate::ui) fn replace_store(
		&mut self,
		store: LocalWorkspaceFacade,
		cache: LocalResourceCache,
		options: SessionOptions,
	) {
		self.app_store.replace_workspace(store, cache, options);
	}
}
