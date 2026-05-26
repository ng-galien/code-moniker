// code-moniker: ignore-file[smell-feature-envy-local, smell-harmonious-method-size]
// TODO(smell): split workspace refresh handling into store-change classification, navigator rebuild, panel refresh, and task scheduling before enabling these guardrails here.
use std::time::Instant;

use crate::perf;
use crate::ui::app::{ActiveFilter, App, ShellAction};
use crate::ui::async_task::TaskSpec;
use crate::ui::live::StoreEvent;
use crate::ui::store::navigation::NavigationAction;
use crate::ui::store::navigation_tree::{build_change_navigator, build_navigator};
use crate::ui::workspace_read::WorkspaceRead;

#[derive(Clone, Copy)]
enum StoreChangeKind {
	FileCatalog,
	Reloaded,
	GitOverlay,
}

impl App {
	pub(in crate::ui) fn handle_store_event(&mut self, event: StoreEvent) {
		if self.queue_store_task(event) {
			return;
		}
		self.handle_store_event_sync(event);
	}

	fn queue_store_task(&mut self, event: StoreEvent) -> bool {
		let task = match event {
			StoreEvent::GitOverlay => TaskSpec::reload_store(self.store_options()),
			StoreEvent::FullIndex => TaskSpec::reload_store(self.store_options()),
		};
		self.queue_task(task)
	}

	pub(in crate::ui) fn handle_store_event_sync(&mut self, event: StoreEvent) {
		match event {
			StoreEvent::GitOverlay => {
				let _ = crate::ui::workspace_read::refresh_workspace(self.store_mut());
				self.apply_refreshed_change_store("git overlay refreshed".to_string());
			}
			StoreEvent::FullIndex => {
				match crate::ui::workspace_read::refresh_workspace(self.store_mut()) {
					Ok(()) => {
						self.apply_reloaded_store(
							"store reloaded after filesystem change".to_string(),
						);
					}
					Err(error) => {
						self.set_status(format!("store reload failed: {error:#}"));
					}
				}
			}
		}
	}

	pub(in crate::ui) fn apply_file_catalog_store(&mut self, status: String) {
		self.refresh_ui_after_store_change(StoreChangeKind::FileCatalog, status);
	}

	pub(in crate::ui) fn apply_reloaded_store(&mut self, status: String) {
		self.refresh_ui_after_store_change(StoreChangeKind::Reloaded, status);
	}

	pub(in crate::ui) fn apply_refreshed_change_store(&mut self, status: String) {
		self.refresh_ui_after_store_change(StoreChangeKind::GitOverlay, status);
	}

	fn refresh_ui_after_store_change(&mut self, kind: StoreChangeKind, status: String) {
		let total_started = Instant::now();
		if matches!(
			kind,
			StoreChangeKind::FileCatalog | StoreChangeKind::Reloaded
		) {
			let started = Instant::now();
			self.watch_roots_update = Some(self.store_watch_roots());
			self.refresh_header_search_options();
			perf::record(
				"store_refresh.watch_search",
				started.elapsed(),
				status.as_str(),
			);
		}
		let reset = matches!(self.active_filter(), ActiveFilter::Change)
			&& self.app_store.navigation().primary_view().rows.is_empty();
		if matches!(kind, StoreChangeKind::Reloaded) {
			let started = Instant::now();
			self.refresh_active_filter_after_store_reload();
			perf::record(
				"store_refresh.active_filter",
				started.elapsed(),
				status.as_str(),
			);
		}
		let started = Instant::now();
		let explorer = build_navigator(self.store());
		perf::record(
			"store_refresh.build_navigator",
			started.elapsed(),
			status.as_str(),
		);
		let started = Instant::now();
		let change = build_change_navigator(self.store());
		perf::record(
			"store_refresh.build_change_navigator",
			started.elapsed(),
			status.as_str(),
		);
		let started = Instant::now();
		self.dispatch_navigation(NavigationAction::ReplaceModels { explorer, change });
		perf::record(
			"store_refresh.replace_models",
			started.elapsed(),
			status.as_str(),
		);
		let started = Instant::now();
		self.refresh_results(matches!(kind, StoreChangeKind::FileCatalog) || reset);
		perf::record(
			"store_refresh.refresh_results",
			started.elapsed(),
			status.as_str(),
		);
		if reset && !matches!(kind, StoreChangeKind::FileCatalog) {
			let started = Instant::now();
			self.select_first_change();
			perf::record(
				"store_refresh.select_first_change",
				started.elapsed(),
				status.as_str(),
			);
		}
		let started = Instant::now();
		self.sync_contextual_view();
		perf::record(
			"store_refresh.sync_contextual_view",
			started.elapsed(),
			status.as_str(),
		);
		self.set_status(status);
		perf::record(
			"store_refresh.total",
			total_started.elapsed(),
			self.status(),
		);
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
		let Some(focus) = self
			.store()
			.usage_focus_for_target(focus.target, focus.label)
		else {
			return;
		};
		let visible_defs = focus.contexts.clone();
		let expand_symbols = visible_defs.len() <= 200;
		self.dispatch_shell(ShellAction::SetUsageLens(Some(focus)));
		self.dispatch_navigation(NavigationAction::SetUsageLens {
			visible_defs,
			reset_expansion: false,
			expand_symbols,
		});
	}
}
