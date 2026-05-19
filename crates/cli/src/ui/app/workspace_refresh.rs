use crate::ui::app::{ActiveFilter, App, ShellAction};
use crate::ui::async_task::TaskSpec;
use crate::ui::live::StoreEvent;
use crate::ui::store::navigation::NavigationAction;
use crate::ui::store::navigation_tree::{build_change_navigator, build_navigator};

impl App {
	pub(in crate::ui) fn handle_store_event(&mut self, event: StoreEvent) {
		if self.queue_store_task(event) {
			return;
		}
		self.handle_store_event_sync(event);
	}

	fn queue_store_task(&mut self, event: StoreEvent) -> bool {
		let task = match event {
			StoreEvent::GitOverlay => {
				TaskSpec::refresh_git_overlay(self.store().git_overlay_refresh_input())
			}
			StoreEvent::FullIndex => TaskSpec::reload_store(self.store().options()),
		};
		self.queue_task(task)
	}

	pub(in crate::ui) fn handle_store_event_sync(&mut self, event: StoreEvent) {
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

	pub(in crate::ui) fn apply_file_catalog_store(&mut self, status: String) {
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

	pub(in crate::ui) fn apply_reloaded_store(&mut self, status: String) {
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

	pub(in crate::ui) fn apply_refreshed_change_store(&mut self, status: String) {
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
}
