use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use code_moniker_core::lang::Lang;
use crossterm::event::KeyEvent;

use crate::Exit;
use crate::args::UiArgs;
use crate::workspace::{
	ChangeDetail, DefLocation, IndexStore, SessionOptions, StoreWatchRoot, UsageFocus,
	WorkspaceStore,
};

mod app;
mod clipboard;
mod component;
mod events;
mod features;
mod kinds;
mod live;
mod navigator;
mod panel;
mod reactive;
mod route;
mod runtime;
mod scroll;
mod shell;
mod source;
mod store;
mod text;
mod theme;
mod view;

use app::{
	ActiveFilter, AppAction, AppStore, ChangePanelMode, CheckState, Effect, FocusRegion,
	HeaderKindFilter, HeaderSearchResults, HeaderSearchState, PanelNavigationState, PanelPolicy,
	ShellAction, TaskCompletion, View, VisualizationMode,
};
use events::{HeaderSearchFocus, UiMode, key_to_msg};
use features::explorer::{
	ExplorerFeature, header_search_options, header_search_results as explorer_header_search_results,
};
use live::StoreEvent;
use navigator::{NavNodeKind, NavRow, build_change_navigator, build_navigator};
use route::Route;
use runtime::{TaskOutcome, TaskRuntime};
use shell::ShellEvent;
use store::navigation::{
	NavigationAction, NavigationNotice, NavigationPane, NavigationScope, NavigationSelection,
	NavigationState, TreePaneAction,
};

const DEFAULT_PANEL_SNAPSHOT_WIDTH: usize = 100;
const HEADER_SEARCH_DEBOUNCE_MS: u64 = 180;

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

pub fn run<W1: Write, W2: Write>(args: &UiArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	shell::terminal::run(args, stdout, stderr)
}

impl ActiveFilter {
	fn filters_navigator(&self) -> bool {
		matches!(self, Self::HeaderSearch(_) | Self::Change)
	}
}

struct App {
	app_store: AppStore,
	scheme: String,
	rules: PathBuf,
	profile: Option<String>,
	event_tx: Option<Sender<ShellEvent>>,
	startup_load_pending: bool,
	watch_roots_update: Option<Vec<StoreWatchRoot>>,
}

impl App {
	fn new(store: WorkspaceStore, scheme: String, rules: PathBuf, profile: Option<String>) -> Self {
		let route = ExplorerFeature::initial_route();
		let navigator = build_navigator(&store);
		let change_navigator = build_change_navigator(&store);
		let mut app_store = AppStore::from_workspace_store(store);
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
		app.dispatch_shell(ShellAction::SetRoute(route));
		app.set_status(format!(
			"Enter opens nodes, Esc/left closes, PgUp/PgDn scroll panel, s focuses search, x resets filters, d changes, u usages, y copies panel, c checks, q quits"
		));
		app.refresh_results(false);
		app
	}

	fn boot(opts: SessionOptions, scheme: String, rules: PathBuf, profile: Option<String>) -> Self {
		let mut app = Self::new(WorkspaceStore::empty(opts), scheme, rules, profile);
		app.startup_load_pending = true;
		app.set_status("loading index...");
		app
	}

	fn status(&self) -> &str {
		self.app_store.status()
	}

	fn set_status(&mut self, status: impl Into<String>) {
		self.dispatch_shell(ShellAction::SetStatus(status.into()));
	}

	fn append_status(&mut self, status: impl AsRef<str>) {
		self.dispatch_shell(ShellAction::AppendStatus(status.as_ref().to_string()));
	}

	fn check_state(&self) -> &CheckState {
		self.app_store.check_state()
	}

	fn set_check_state(&mut self, state: CheckState) {
		self.dispatch_shell(ShellAction::SetCheckState(state));
	}

	fn dispatch_shell(&mut self, action: ShellAction) {
		let refresh_search_options = matches!(
			action,
			ShellAction::SetHeaderSearchFilters { .. } | ShellAction::ClearFilter { .. }
		);
		self.dispatch_and_apply(&AppAction::Shell(action));
		if refresh_search_options {
			self.refresh_header_search_options();
		}
	}

	fn view(&self) -> View {
		self.app_store.shell().view
	}

	fn view_mode(&self) -> VisualizationMode {
		self.app_store.shell().view_mode
	}

	fn panel_policy(&self) -> PanelPolicy {
		self.app_store.shell().panel_policy
	}

	fn change_panel(&self) -> ChangePanelMode {
		self.app_store.shell().change_panel
	}

	fn mode(&self) -> UiMode {
		self.app_store.shell().mode
	}

	fn focus_region(&self) -> FocusRegion {
		self.app_store.shell().focus_region
	}

	fn usage_lens(&self) -> Option<&UsageFocus> {
		self.app_store.shell().usage_lens.as_ref()
	}

	fn active_filter(&self) -> &ActiveFilter {
		&self.app_store.shell().active_filter
	}

	fn header_search(&self) -> &HeaderSearchState {
		&self.app_store.shell().header_search
	}

	fn store(&self) -> &WorkspaceStore {
		self.app_store.workspace()
	}

	fn store_mut(&mut self) -> &mut WorkspaceStore {
		self.app_store.workspace_mut()
	}

	fn replace_store(&mut self, store: WorkspaceStore) {
		self.app_store.replace_workspace(store);
	}

	fn selected(&self) -> Option<DefLocation> {
		self.selected_nav_row().and_then(|row| match row.kind {
			NavNodeKind::Def(loc) => Some(loc),
			_ => None,
		})
	}

	fn primary_selected(&self) -> Option<DefLocation> {
		self.primary_selected_nav_row()
			.and_then(|row| match row.kind {
				NavNodeKind::Def(loc) => Some(loc),
				_ => None,
			})
	}

	fn selected_change_detail(&self) -> Option<ChangeDetail> {
		self.selected_nav_row().and_then(|row| match row.kind {
			NavNodeKind::Change(id) => self.store().change_detail(id),
			NavNodeKind::Def(loc) => self.store().change_detail_for_symbol(&loc),
			_ => None,
		})
	}

	fn selected_nav_row(&self) -> Option<&NavRow> {
		self.app_store
			.navigation()
			.pane_view(self.active_navigation_pane())
			.and_then(|pane| pane.selected_row())
	}

	fn primary_selected_nav_row(&self) -> Option<&NavRow> {
		self.app_store.navigation().primary_view().selected_row()
	}

	fn active_navigation_pane(&self) -> NavigationPane {
		if self.focus_region() == FocusRegion::UsageLens {
			NavigationPane::UsageLens
		} else {
			NavigationPane::Primary
		}
	}

	fn panel_scroll(&self) -> usize {
		self.app_store.shell().panel_navigation.scroll
	}

	fn panel_navigation(&self) -> &PanelNavigationState {
		&self.app_store.shell().panel_navigation
	}

	fn reset_panel_navigation(&mut self) {
		if self.app_store.shell().panel_navigation == PanelNavigationState::default() {
			return;
		}
		self.dispatch_shell(ShellAction::SetPanelNavigation(
			PanelNavigationState::default(),
		));
	}

	fn dispatch_navigation(&mut self, action: NavigationAction) -> bool {
		let before = self.selected_nav_row().map(|row| row.key.clone());
		let (changed, effects) = {
			let transition = self.app_store.dispatch_navigation(action);
			(transition.changed, transition.take_effects())
		};
		self.apply_effects(effects);
		if changed && before != self.selected_nav_row().map(|row| row.key.clone()) {
			self.reset_panel_navigation();
		}
		changed
	}

	fn refresh_results(&mut self, reset_expansion: bool) {
		let visible_defs = self.matching_defs();
		let expand_symbols = visible_defs.len() <= 200;
		self.dispatch_navigation(NavigationAction::SetScope {
			scope: self.navigation_scope(),
			visible_defs,
			reset_expansion,
			expand_symbols,
		});
	}

	fn matching_defs(&self) -> Vec<DefLocation> {
		match self.active_filter() {
			ActiveFilter::HeaderSearch(results) => results.matches.clone(),
			ActiveFilter::Change => self.store().changed_defs(),
			ActiveFilter::None => self.store().all_navigable_defs(),
		}
	}

	fn navigation_scope(&self) -> NavigationScope {
		if matches!(self.active_filter(), ActiveFilter::Change) {
			NavigationScope::Change
		} else if self.is_filtered() {
			NavigationScope::Filtered
		} else {
			NavigationScope::Explorer
		}
	}

	fn select_def(&mut self, loc: DefLocation) {
		self.dispatch_navigation(primary_tree_selection(NavigationSelection::Def(loc)));
	}

	fn select_first_change(&mut self) {
		self.dispatch_navigation(primary_tree_selection(NavigationSelection::FirstChange));
	}

	fn filter_label(&self) -> String {
		if matches!(self.mode(), UiMode::HeaderSearch(_)) {
			let header = self.header_search();
			return header_search_label(&header.text, &header.langs, &header.kind_filters);
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

	fn is_filtered(&self) -> bool {
		self.active_filter().filters_navigator()
	}

	fn has_clearable_scope(&self) -> bool {
		!matches!(self.active_filter(), ActiveFilter::None) || self.usage_lens().is_some()
	}

	fn contextual_view(&self) -> View {
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

	fn sync_contextual_view(&mut self) {
		if self.panel_policy() == PanelPolicy::Contextual {
			self.set_view(self.contextual_view(), PanelPolicy::Contextual);
		}
	}

	fn set_view(&mut self, view: View, policy: PanelPolicy) {
		self.dispatch_shell(ShellAction::SetView {
			view,
			policy,
			route: view.route(),
		});
	}

	fn scope_label(&self) -> String {
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

	fn focus_usages(&mut self, loc: DefLocation) {
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

	fn apply_header_search(&mut self, generation: Option<u64>, return_focus: bool) {
		if generation.is_some() && generation != self.header_search().pending_generation {
			return;
		}
		let header = self.header_search().clone();
		if !header.has_filter() {
			self.clear_filter_with_focus(return_focus);
			if return_focus {
				self.dispatch_shell(ShellAction::SetStatus("search cleared".to_string()));
			}
			return;
		}
		let results = self.header_search_results(&header.text, &header.langs, &header.kind_filters);
		let match_count = results.matches.len();
		let first_match = results.matches.first().copied();
		self.dispatch_shell(ShellAction::ApplyHeaderSearch {
			results: results.clone(),
			return_focus,
		});
		self.refresh_results(true);
		if let Some(loc) = first_match {
			self.select_def(loc);
		}
		self.sync_contextual_view();
		if return_focus {
			self.dispatch_shell(ShellAction::SetStatus(format!(
				"search applied: {} ({}/{})",
				results.label(),
				match_count,
				self.store().stats().defs
			)));
		} else {
			self.set_status(format!(
				"search: {} ({}/{})",
				results.label(),
				match_count,
				self.store().stats().defs
			));
		}
	}

	fn header_search_results(
		&self,
		text: &str,
		langs: &[Lang],
		kind_filters: &[HeaderKindFilter],
	) -> HeaderSearchResults {
		explorer_header_search_results(self.store(), text, langs, kind_filters)
	}

	fn cycle_header_search_selector(&mut self, direction: i8) {
		let focus = match self.mode() {
			UiMode::HeaderSearch(focus) => focus,
			UiMode::Normal => HeaderSearchFocus::Text,
		};
		match focus {
			HeaderSearchFocus::Text => {
				self.dispatch_shell(ShellAction::SetStatus(
					"type text or press Tab to edit language".to_string(),
				));
			}
			HeaderSearchFocus::Lang => {
				if !self.header_search().combo_open {
					self.set_status("press Enter to open the selector, Space toggles an option");
					return;
				}
				let options = self.available_header_langs();
				let cursor = cycle_index(
					self.header_search().lang_cursor,
					options.len() + 1,
					direction,
				);
				self.dispatch_shell(ShellAction::SetHeaderSearchCursor {
					focus: HeaderSearchFocus::Lang,
					cursor,
				});
				self.set_status(format!(
					"language option: {}",
					lang_selector_option_label(&self.header_search().langs, &options, cursor)
				));
			}
			HeaderSearchFocus::Kind => {
				let options = self.available_header_kind_filters();
				let cursor = cycle_index(
					self.header_search().kind_cursor,
					options.len() + 1,
					direction,
				);
				self.dispatch_shell(ShellAction::SetHeaderSearchCursor {
					focus: HeaderSearchFocus::Kind,
					cursor,
				});
				self.set_status(format!(
					"kind option: {}",
					kind_selector_option_label(
						&self.header_search().kind_filters,
						&options,
						cursor
					)
				));
			}
		}
	}

	fn toggle_header_search_selection(&mut self) {
		let focus = match self.mode() {
			UiMode::HeaderSearch(focus) => focus,
			UiMode::Normal => HeaderSearchFocus::Text,
		};
		match focus {
			HeaderSearchFocus::Text => {
				self.apply_header_search(None, true);
			}
			HeaderSearchFocus::Lang => {
				if !self.header_search().combo_open {
					self.set_status("press Enter to open the selector, Space toggles an option");
					return;
				}
				let options = self.available_header_langs();
				let cursor = self.header_search().lang_cursor.min(options.len());
				let mut langs = self.header_search().langs.clone();
				if cursor == 0 {
					langs.clear();
				} else {
					toggle_value(&mut langs, options[cursor - 1]);
				}
				self.dispatch_shell(ShellAction::SetHeaderSearchFilters {
					langs: langs.clone(),
					kind_filters: self.header_search().kind_filters.clone(),
				});
				self.set_status(format!("language filter: {}", lang_filter_summary(&langs)));
			}
			HeaderSearchFocus::Kind => {
				if !self.header_search().combo_open {
					self.set_status("press Enter to open the selector, Space toggles an option");
					return;
				}
				let options = self.available_header_kind_filters();
				let cursor = self.header_search().kind_cursor.min(options.len());
				let mut filters = self.header_search().kind_filters.clone();
				if cursor == 0 {
					filters.clear();
				} else {
					toggle_value(&mut filters, options[cursor - 1].clone());
				}
				self.dispatch_shell(ShellAction::SetHeaderSearchFilters {
					langs: self.header_search().langs.clone(),
					kind_filters: filters.clone(),
				});
				self.set_status(format!("kind filter: {}", kind_filter_summary(&filters)));
			}
		}
	}

	fn available_header_langs(&self) -> Vec<Lang> {
		self.header_search().available_langs.clone()
	}

	fn available_header_kind_filters(&self) -> Vec<HeaderKindFilter> {
		self.header_search().available_kind_filters.clone()
	}

	fn refresh_header_search_options(&mut self) {
		let options = header_search_options(self.store(), self.header_search());
		self.dispatch_and_apply(&AppAction::Shell(ShellAction::SetHeaderSearchOptions {
			langs: options.langs,
			kind_filters: options.kind_filters,
			available_langs: options.available_langs,
			available_kind_filters: options.available_kind_filters,
			lang_cursor: options.lang_cursor,
			kind_cursor: options.kind_cursor,
		}));
	}

	fn clear_filter(&mut self) {
		self.clear_filter_with_focus(true);
	}

	fn clear_filter_with_focus(&mut self, return_focus: bool) {
		self.dispatch_shell(ShellAction::ClearFilter { return_focus });
		self.dispatch_navigation(NavigationAction::ClearUsageLens);
		self.refresh_results(true);
		self.sync_contextual_view();
		self.set_status("filter cleared");
	}

	fn focus_usages_of_selected(&mut self) {
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

	fn close_usage_lens(&mut self) {
		let label = self
			.usage_lens()
			.map(|focus| focus.label.clone())
			.unwrap_or_else(|| "usage lens".to_string());
		self.dispatch_shell(ShellAction::SetUsageLens(None));
		self.dispatch_navigation(NavigationAction::ClearUsageLens);
		self.sync_contextual_view();
		self.set_status(format!("closed usage lens for {label}"));
	}

	fn toggle_change_mode(&mut self) {
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

	fn toggle_change_usages(&mut self) {
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

	fn handle_store_event(&mut self, event: StoreEvent) {
		if self.queue_store_task(event) {
			return;
		}
		self.handle_store_event_sync(event);
	}

	fn queue_store_task(&mut self, event: StoreEvent) -> bool {
		let task = match event {
			StoreEvent::GitOverlay => {
				runtime::TaskSpec::refresh_git_overlay(self.store().git_overlay_refresh_input())
			}
			StoreEvent::FullIndex => runtime::TaskSpec::reload_store(self.store().options()),
		};
		self.queue_task(task)
	}

	fn handle_store_event_sync(&mut self, event: StoreEvent) {
		match event {
			StoreEvent::GitOverlay => {
				self.store_mut().refresh_git_overlay();
				self.apply_refreshed_change_store("git overlay refreshed".to_string());
			}
			StoreEvent::FullIndex => match self.store_mut().reload() {
				Ok(()) => {
					self.apply_reloaded_store("store reloaded after filesystem change".to_string());
				}
				Err(error) => {
					self.set_status(format!("store reload failed: {error:#}"));
				}
			},
		}
	}

	fn apply_file_catalog_store(&mut self, status: String) {
		self.watch_roots_update = Some(self.store().watch_roots());
		self.refresh_header_search_options();
		self.dispatch_navigation(NavigationAction::ReplaceModels {
			explorer: build_navigator(self.store()),
			change: build_change_navigator(self.store()),
		});
		self.refresh_results(true);
		self.sync_contextual_view();
		self.set_status(status);
	}

	fn apply_reloaded_store(&mut self, status: String) {
		self.watch_roots_update = Some(self.store().watch_roots());
		self.refresh_header_search_options();
		let reset = matches!(self.active_filter(), ActiveFilter::Change)
			&& self.app_store.navigation().primary_view().rows.is_empty();
		self.refresh_active_filter_after_store_reload();
		self.dispatch_navigation(NavigationAction::ReplaceModels {
			explorer: build_navigator(self.store()),
			change: build_change_navigator(self.store()),
		});
		self.refresh_results(reset);
		if reset {
			self.select_first_change();
		}
		self.sync_contextual_view();
		self.set_status(status);
	}

	fn apply_refreshed_change_store(&mut self, status: String) {
		let reset = matches!(self.active_filter(), ActiveFilter::Change)
			&& self.app_store.navigation().primary_view().rows.is_empty();
		self.dispatch_navigation(NavigationAction::ReplaceModels {
			explorer: build_navigator(self.store()),
			change: build_change_navigator(self.store()),
		});
		self.refresh_results(reset);
		if reset {
			self.select_first_change();
		}
		self.sync_contextual_view();
		self.set_status(status);
	}

	fn refresh_active_filter_after_store_reload(&mut self) {
		let active_filter = match self.active_filter() {
			ActiveFilter::HeaderSearch(results) => ActiveFilter::HeaderSearch(
				self.header_search_results(&results.text, &results.langs, &results.kind_filters),
			),
			ActiveFilter::None => ActiveFilter::None,
			ActiveFilter::Change => ActiveFilter::Change,
		};
		self.dispatch_shell(ShellAction::ReplaceActiveFilter(active_filter));
		self.refresh_usage_lens_after_store_reload();
	}

	fn refresh_usage_lens_after_store_reload(&mut self) {
		let Some(focus) = self.usage_lens().cloned() else {
			return;
		};
		let focus = self
			.store()
			.usage_focus_for_target(focus.target, focus.label);
		let visible_defs = focus.contexts.clone();
		let expand_symbols = visible_defs.len() <= 200;
		self.dispatch_shell(ShellAction::SetUsageLens(Some(focus)));
		self.dispatch_navigation(NavigationAction::SetUsageLens {
			visible_defs,
			reset_expansion: false,
			expand_symbols,
		});
	}

	fn toggle_selected_nav(&mut self) {
		self.dispatch_navigation(focused_tree_action(
			self.focus_region(),
			TreePaneAction::ToggleSelected,
		));
		match self.app_store.navigation().last_notice() {
			NavigationNotice::Opened(label) => self.set_status(format!("opened {label}")),
			NavigationNotice::Closed(label) => self.set_status(format!("closed {label}")),
			NavigationNotice::MovedToParent | NavigationNotice::Noop => {}
		}
	}

	fn open_selected_nav(&mut self) {
		self.dispatch_navigation(focused_tree_action(
			self.focus_region(),
			TreePaneAction::OpenSelected,
		));
		if let NavigationNotice::Opened(label) = self.app_store.navigation().last_notice() {
			self.set_status(format!("opened {label}"));
		}
	}

	fn close_selected_nav(&mut self) -> bool {
		self.dispatch_navigation(focused_tree_action(
			self.focus_region(),
			TreePaneAction::CloseSelected,
		));
		match self.app_store.navigation().last_notice() {
			NavigationNotice::Closed(label) => {
				self.set_status(format!("closed {label}"));
				true
			}
			NavigationNotice::MovedToParent => {
				self.sync_contextual_view();
				true
			}
			NavigationNotice::Opened(_) => false,
			NavigationNotice::Noop if self.focus_region() == FocusRegion::UsageLens => {
				self.dispatch_shell(ShellAction::SetFocusRegion(FocusRegion::Navigator));
				self.sync_contextual_view();
				self.set_status("navigator focused");
				true
			}
			NavigationNotice::Noop => false,
		}
	}

	fn run_check(&mut self) {
		self.set_view(View::Check, PanelPolicy::Manual);
		let task = runtime::TaskSpec::run_check(
			self.store().clone(),
			self.rules.clone(),
			self.profile.clone(),
			self.scheme.clone(),
		);
		if self.queue_task(task) {
			self.set_status("check queued in background");
			return;
		}
		match self
			.store()
			.check_summary(&self.rules, self.profile.as_deref(), &self.scheme)
		{
			Ok(summary) => {
				self.set_status(format!(
					"check complete: {} violation(s) across {} file(s)",
					summary.total_violations, summary.files_with_violations
				));
				self.set_check_state(CheckState::Ready(summary));
			}
			Err(e) => {
				self.set_status("check failed");
				self.set_check_state(CheckState::Error(e.to_string()));
			}
		}
	}

	fn set_event_sender(&mut self, tx: Sender<ShellEvent>) {
		self.event_tx = Some(tx);
	}

	fn queue_startup_load(&mut self) {
		if !self.startup_load_pending {
			return;
		}
		self.startup_load_pending = false;
		if self.queue_task(runtime::TaskSpec::load_file_catalog(self.store().options())) {
			self.set_status("loading file tree in background");
		} else {
			self.handle_store_event_sync(StoreEvent::FullIndex);
		}
	}

	fn take_watch_roots_update(&mut self) -> Option<Vec<StoreWatchRoot>> {
		self.watch_roots_update.take()
	}

	fn handle_clipboard_result(&mut self, result: clipboard::ClipboardResult) {
		match result.result {
			Ok(()) => {
				self.set_status(format!("copied {} snapshot to clipboard", result.component));
			}
			Err(error) => {
				self.set_status(format!(
					"clipboard copy failed for {}: {error}",
					result.component
				));
			}
		}
	}

	fn handle_task_result(&mut self, result: runtime::TaskResult) {
		match result.outcome {
			TaskOutcome::FileCatalogLoaded(store) => {
				self.replace_store(*store);
				self.apply_file_catalog_store("file tree ready".to_string());
				if self.queue_task(runtime::TaskSpec::reload_store(self.store().options())) {
					self.set_status("file tree ready; loading symbols in background");
				}
			}
			TaskOutcome::StoreReloaded(store) => {
				self.replace_store(*store);
				self.apply_reloaded_store(format!("{} completed", result.label));
			}
			TaskOutcome::GitOverlayRefreshed(store) => {
				if self.store_mut().apply_git_overlay_refresh(*store) {
					self.apply_refreshed_change_store(format!("{} completed", result.label));
				} else {
					self.set_status(format!("ignored stale {} result", result.label));
				}
			}
			TaskOutcome::CheckCompleted(summary) => {
				self.set_status(format!(
					"check complete: {} violation(s) across {} file(s)",
					summary.total_violations, summary.files_with_violations
				));
			}
			TaskOutcome::Failed(error) => {
				self.set_status(format!("{} failed: {error}", result.label));
			}
		}
	}

	fn copy_panel_snapshot(&mut self) {
		let panel = ExplorerFeature::active_panel(self);
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

	fn handle_key(&mut self, key: KeyEvent) -> anyhow::Result<bool> {
		Ok(self.update(AppAction::Ui(key_to_msg(self.mode(), key))))
	}

	fn update(&mut self, action: AppAction) -> bool {
		let action = match action {
			AppAction::TaskCompleted(result) => {
				match self.app_store.complete_task(&result) {
					TaskCompletion::Accepted => self.handle_task_result(result),
					TaskCompletion::Ignored => {
						self.set_status(format!("ignored stale task result: {}", result.label));
					}
				}
				return false;
			}
			action => action,
		};
		if self.dispatch_and_apply(&action) {
			return true;
		}
		match action {
			AppAction::Ui(_) => false,
			AppAction::HeaderSearchDebounced(_) => false,
			AppAction::Shell(_) => false,
			AppAction::Store(event) => {
				self.handle_store_event(event);
				false
			}
			AppAction::TaskStarted { .. } => false,
			AppAction::TaskCompleted(_) => unreachable!("task completion handled before dispatch"),
			AppAction::Clipboard(result) => {
				self.handle_clipboard_result(result);
				false
			}
		}
	}

	fn dispatch_and_apply(&mut self, action: &AppAction) -> bool {
		let effects = {
			let transition = self.app_store.dispatch(action);
			transition.take_effects()
		};
		self.apply_effects(effects)
	}

	fn apply_effects(&mut self, effects: Vec<Effect>) -> bool {
		for effect in effects {
			if self.apply_effect(effect) {
				return true;
			}
		}
		false
	}

	fn apply_effect(&mut self, effect: Effect) -> bool {
		match effect {
			Effect::Navigate(route) => self.navigate(route),
			Effect::Quit => return true,
			Effect::DebounceHeaderSearch(generation) => {
				self.queue_header_search_debounce(generation);
			}
			Effect::ApplyHeaderSearch {
				generation,
				return_focus,
			} => self.apply_header_search(generation, return_focus),
			Effect::CycleHeaderSearchSelector { direction } => {
				self.cycle_header_search_selector(direction)
			}
			Effect::ToggleHeaderSearchSelection => self.toggle_header_search_selection(),
			Effect::FocusUsages => self.focus_usages_of_selected(),
			Effect::ToggleChangeMode => self.toggle_change_mode(),
			Effect::CopyPanelSnapshot => self.copy_panel_snapshot(),
			Effect::RunCheck => self.run_check(),
			Effect::Navigation(action) => self.apply_navigation(*action),
			Effect::ToggleFocusRegion => self.toggle_focus_region(),
			Effect::PanelMove { direction } => self.move_panel_selection(direction),
			Effect::PanelHome => self.move_panel_to_edge(true),
			Effect::PanelEnd => self.move_panel_to_edge(false),
			Effect::ToggleSelectedNode => self.toggle_selected_nav(),
			Effect::OpenSelectedNode => self.open_selected_nav(),
			Effect::CloseNodeOrClearScope => {
				if !self.close_selected_nav() && self.has_clearable_scope() {
					self.clear_filter();
				}
			}
		}
		false
	}

	fn toggle_focus_region(&mut self) {
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

	fn ensure_active_panel_selection(&mut self) {
		let panel = ExplorerFeature::active_panel(self);
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

	fn move_panel_selection(&mut self, direction: i8) {
		let panel = ExplorerFeature::active_panel(self);
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

	fn move_panel_to_edge(&mut self, home: bool) {
		let panel = ExplorerFeature::active_panel(self);
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

	fn scroll_panel_lines(&mut self, direction: i8) {
		let next = if direction > 0 {
			self.panel_scroll().saturating_add(1)
		} else {
			self.panel_scroll().saturating_sub(1)
		};
		self.dispatch_shell(ShellAction::SetPanelScroll(next));
	}

	fn apply_navigation(&mut self, action: NavigationAction) {
		let changed = self.dispatch_navigation(action);
		if changed {
			self.sync_contextual_view();
		}
	}

	fn queue_task(&mut self, task: runtime::TaskSpec) -> bool {
		let Some(tx) = self.event_tx.clone() else {
			let label = task.label().to_string();
			self.set_status(format!("task runtime unavailable for {label}"));
			return false;
		};
		let task = self.app_store.register_task(task);
		let label = task.label().to_string();
		let id = task.id();
		TaskRuntime::spawn(task, move |result| {
			let _ = tx.send(ShellEvent::TaskCompleted(result));
		});
		self.set_status(format!("task queued: {label} ({id})"));
		true
	}

	fn queue_header_search_debounce(&mut self, generation: u64) {
		let Some(tx) = self.event_tx.clone() else {
			return;
		};
		thread::spawn(move || {
			thread::sleep(Duration::from_millis(HEADER_SEARCH_DEBOUNCE_MS));
			let _ = tx.send(ShellEvent::HeaderSearchDebounced(generation));
		});
	}

	fn navigate(&mut self, route: Route) {
		if !ExplorerFeature::can_open(&route) {
			self.set_status(format!("unknown route: {}/{}", route.feature, route.path));
			return;
		}
		if let Some(view) = View::from_route_path(&route.path) {
			self.set_view(view, PanelPolicy::Manual);
			return;
		}
		self.dispatch_shell(ShellAction::SetRoute(route));
	}
}

fn display_filter(filter: &str) -> &str {
	if filter.is_empty() { "all" } else { filter }
}

fn header_search_label(text: &str, langs: &[Lang], kind_filters: &[HeaderKindFilter]) -> String {
	let mut parts = Vec::new();
	if !text.trim().is_empty() {
		parts.push(format!("search:{}", text.trim()));
	}
	if !langs.is_empty() {
		parts.push(format!("lang:{}", lang_filter_summary(langs)));
	}
	if !kind_filters.is_empty() {
		parts.push(format!("kind:{}", kind_filter_summary(kind_filters)));
	}
	if parts.is_empty() {
		"<all>".to_string()
	} else {
		parts.join(" ")
	}
}

fn lang_filter_summary(langs: &[Lang]) -> String {
	if langs.is_empty() {
		return "<all>".to_string();
	}
	langs
		.iter()
		.map(|lang| lang.tag())
		.collect::<Vec<_>>()
		.join(",")
}

fn kind_filter_summary(filters: &[HeaderKindFilter]) -> String {
	if filters.is_empty() {
		return "<all>".to_string();
	}
	filters
		.iter()
		.map(HeaderKindFilter::label)
		.collect::<Vec<_>>()
		.join(",")
}

fn lang_selector_option_label(selected: &[Lang], options: &[Lang], cursor: usize) -> String {
	if cursor == 0 {
		return if selected.is_empty() {
			"<all>".to_string()
		} else {
			"clear".to_string()
		};
	}
	let Some(lang) = options.get(cursor - 1).copied() else {
		return "<all>".to_string();
	};
	let marker = if selected.contains(&lang) { "-" } else { "+" };
	format!("{marker}{}", lang.tag())
}

fn kind_selector_option_label(
	selected: &[HeaderKindFilter],
	options: &[HeaderKindFilter],
	cursor: usize,
) -> String {
	if cursor == 0 {
		return if selected.is_empty() {
			"<all>".to_string()
		} else {
			"clear".to_string()
		};
	}
	let Some(filter) = options.get(cursor - 1) else {
		return "<all>".to_string();
	};
	let marker = if selected.contains(filter) { "-" } else { "+" };
	format!("{marker}{}", filter.label())
}

fn cycle_index(current: usize, len: usize, direction: i8) -> usize {
	if len == 0 {
		return 0;
	}
	let current = current.min(len - 1);
	if direction >= 0 {
		(current + 1) % len
	} else {
		(current + len - 1) % len
	}
}

fn toggle_value<T: Eq>(values: &mut Vec<T>, value: T) {
	if let Some(idx) = values.iter().position(|candidate| candidate == &value) {
		values.remove(idx);
	} else {
		values.push(value);
	}
}
