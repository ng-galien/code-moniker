// code-moniker: ignore-file[smell-feature-envy-local, smell-harmonious-method-size]
// TODO(smell): split header-search selector movement, facet selection, query editing, and result application before enabling these guardrails here.
use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;

use crate::ui::app::App;
use crate::ui::events::{HeaderSearchFocus, UiMode};
use crate::ui::explorer::{
	HeaderSearchResults, header_search_options,
	header_search_results as explorer_header_search_results,
};
use crate::ui::store::navigation::{NavigationAction, NavigationScope};
use crate::ui::workspace_state::WorkspaceState;
use code_moniker_workspace::snapshot::SymbolId;
type DefLocation = SymbolId;

use super::action::{AppAction, ShellAction};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct HeaderSearchState {
	pub(in crate::ui) focus: HeaderSearchFocus,
	pub(in crate::ui) text: String,
	pub(in crate::ui) langs: Vec<Lang>,
	pub(in crate::ui) kind_filters: Vec<HeaderKindFilter>,
	pub(in crate::ui) available_langs: Vec<Lang>,
	pub(in crate::ui) available_kind_filters: Vec<HeaderKindFilter>,
	pub(in crate::ui) lang_cursor: usize,
	pub(in crate::ui) kind_cursor: usize,
	pub(in crate::ui) combo_open: bool,
	pub(in crate::ui) generation: u64,
	pub(in crate::ui) pending_generation: Option<u64>,
}

impl HeaderSearchState {
	pub(in crate::ui) fn has_filter(&self) -> bool {
		!self.text.trim().is_empty() || !self.langs.is_empty() || !self.kind_filters.is_empty()
	}

	pub(in crate::ui) fn reset(&mut self) {
		self.text.clear();
		self.langs.clear();
		self.kind_filters.clear();
		self.lang_cursor = 0;
		self.kind_cursor = 0;
		self.combo_open = false;
	}

	pub(in crate::ui) fn bump_pending(&mut self) -> u64 {
		self.generation += 1;
		self.pending_generation = Some(self.generation);
		self.generation
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum HeaderKindFilter {
	Kind(String),
	Shape(Shape),
}

impl HeaderKindFilter {
	pub(in crate::ui) fn label(&self) -> String {
		match self {
			Self::Kind(kind) => kind.clone(),
			Self::Shape(shape) => format!("shape:{}", shape.as_str()),
		}
	}

	pub(in crate::ui) fn matches_kind(&self, kind: &str) -> bool {
		match self {
			Self::Kind(filter) => filter == kind,
			Self::Shape(shape) => shape_of(kind.as_bytes()) == Some(*shape),
		}
	}
}

pub(in crate::ui) fn header_search_label(
	text: &str,
	langs: &[Lang],
	kind_filters: &[HeaderKindFilter],
) -> String {
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

pub(in crate::ui) fn display_filter(filter: &str) -> &str {
	if filter.is_empty() { "all" } else { filter }
}

pub(in crate::ui) fn lang_filter_summary(langs: &[Lang]) -> String {
	if langs.is_empty() {
		return "<all>".to_string();
	}
	langs
		.iter()
		.map(|lang| lang.tag())
		.collect::<Vec<_>>()
		.join(",")
}

pub(in crate::ui) fn kind_filter_summary(filters: &[HeaderKindFilter]) -> String {
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

#[derive(Debug)]
struct HeaderSearchDecision {
	shell: Vec<ShellAction>,
	navigation: Vec<NavigationAction>,
	select: Option<DefLocation>,
	status: Option<String>,
	sync_contextual_view: bool,
}

impl HeaderSearchDecision {
	fn stale() -> Self {
		Self {
			shell: Vec::new(),
			navigation: Vec::new(),
			select: None,
			status: None,
			sync_contextual_view: false,
		}
	}
}

fn decide_apply_header_search(
	header: &HeaderSearchState,
	store: &WorkspaceState,
	generation: Option<u64>,
	return_focus: bool,
) -> HeaderSearchDecision {
	if generation.is_some() && generation != header.pending_generation {
		return HeaderSearchDecision::stale();
	}
	if !header.has_filter() {
		let visible_defs = store.all_navigable_defs();
		let expand_symbols = visible_defs.len() <= 200;
		return HeaderSearchDecision {
			shell: vec![ShellAction::ClearFilter { return_focus }],
			navigation: vec![NavigationAction::SetScope {
				scope: NavigationScope::Explorer,
				visible_defs,
				reset_expansion: true,
				expand_symbols,
			}],
			select: None,
			status: Some(if return_focus {
				"search cleared".to_string()
			} else {
				"filter cleared".to_string()
			}),
			sync_contextual_view: true,
		};
	}

	let results =
		explorer_header_search_results(store, &header.text, &header.langs, &header.kind_filters);
	let match_count = results.matches.len();
	let visible_defs = results.matches.clone();
	let expand_symbols = visible_defs.len() <= 200;
	let select = results.matches.first().cloned();
	let status = if return_focus {
		format!(
			"search applied: {} ({}/{})",
			results.label(),
			match_count,
			store.stats().defs
		)
	} else {
		format!(
			"search: {} ({}/{})",
			results.label(),
			match_count,
			store.stats().defs
		)
	};

	HeaderSearchDecision {
		shell: vec![ShellAction::ApplyHeaderSearch {
			results,
			return_focus,
		}],
		navigation: vec![NavigationAction::SetScope {
			scope: NavigationScope::Filtered,
			visible_defs,
			reset_expansion: true,
			expand_symbols,
		}],
		select,
		status: Some(status),
		sync_contextual_view: true,
	}
}

impl App {
	pub(in crate::ui) fn apply_header_search(
		&mut self,
		generation: Option<u64>,
		return_focus: bool,
	) {
		let decision = decide_apply_header_search(
			self.header_search(),
			self.store(),
			generation,
			return_focus,
		);
		for action in decision.shell {
			self.dispatch_shell(action);
		}
		for action in decision.navigation {
			self.apply_navigation(action);
		}
		if let Some(loc) = decision.select {
			self.select_def(loc);
		}
		if decision.sync_contextual_view {
			self.sync_contextual_view();
		}
		if let Some(status) = decision.status {
			self.set_status(status);
		}
	}

	pub(in crate::ui) fn header_search_results(
		&self,
		text: &str,
		langs: &[Lang],
		kind_filters: &[HeaderKindFilter],
	) -> HeaderSearchResults {
		explorer_header_search_results(self.store(), text, langs, kind_filters)
	}

	pub(in crate::ui) fn cycle_header_search_selector(&mut self, direction: i8) {
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

	pub(in crate::ui) fn toggle_header_search_selection(&mut self) {
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

	pub(in crate::ui) fn available_header_langs(&self) -> Vec<Lang> {
		self.header_search().available_langs.clone()
	}

	pub(in crate::ui) fn available_header_kind_filters(&self) -> Vec<HeaderKindFilter> {
		self.header_search().available_kind_filters.clone()
	}

	pub(in crate::ui) fn refresh_header_search_options(&mut self) {
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

	pub(in crate::ui) fn clear_filter(&mut self) {
		self.clear_filter_with_focus(true);
	}

	pub(in crate::ui) fn clear_filter_with_focus(&mut self, return_focus: bool) {
		self.dispatch_shell(ShellAction::ClearFilter { return_focus });
		self.refresh_results(true);
		self.sync_contextual_view();
		self.set_status("filter cleared");
	}
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use code_moniker_core::lang::Lang;

	use super::*;
	use crate::session::SessionOptions;
	use crate::ui::app::{ActiveFilter, App, VisualizationMode};
	use crate::ui::events::{FilterEdit, Msg};
	use crate::ui::workspace_state::WorkspaceState;

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
			"export class AlphaService {}\nexport class BetaService {}\nexport function betaFactory() { return new BetaService(); }\n",
		);
		write(
			tmp.path(),
			"src/pkg/main.go",
			"package pkg\n\nfunc BuildBeta() {}\n",
		);
		let store = WorkspaceState::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		App::new(
			store,
			"default".to_string(),
			tmp.path().join("rules.toml"),
			None,
		)
	}

	fn input_search(app: &mut App, text: &str) -> u64 {
		app.update(crate::ui::app::AppAction::Ui(Msg::ToggleHeaderSearch));
		let mut generation = app.header_search().generation;
		for ch in text.chars() {
			app.update(crate::ui::app::AppAction::Ui(Msg::HeaderSearchInput(
				FilterEdit::Push(ch),
			)));
			generation = app.header_search().pending_generation.unwrap();
		}
		generation
	}

	fn selected_name(app: &App) -> Option<String> {
		app.selected()
			.map(|loc| app.store().symbol_summary(&loc).name)
	}

	fn primary_selected_name(app: &App) -> Option<String> {
		app.primary_selected()
			.map(|loc| app.store().symbol_summary(&loc).name)
	}

	fn def_named(app: &App, name: &str) -> DefLocation {
		app.store()
			.all_navigable_defs()
			.into_iter()
			.find(|loc| app.store().symbol_summary(loc).name == name)
			.unwrap_or_else(|| panic!("missing def {name}"))
	}

	#[test]
	fn empty_header_search_clears_filter_and_restores_unfiltered_results() {
		let mut app = fixture_app();
		let generation = input_search(&mut app, "Alpha");
		app.update(crate::ui::app::AppAction::HeaderSearchDebounced(generation));
		assert!(matches!(app.active_filter(), ActiveFilter::HeaderSearch(_)));

		app.update(crate::ui::app::AppAction::Ui(Msg::HeaderSearchReset));

		assert!(matches!(app.active_filter(), ActiveFilter::None));
		assert_eq!(app.view_mode(), VisualizationMode::Explorer);
		assert!(!app.is_filtered());
		assert_eq!(
			app.navigation().visible_defs().len(),
			app.store().all_navigable_defs().len()
		);
	}

	#[test]
	fn applying_header_search_selects_the_first_match() {
		let mut app = fixture_app();
		let generation = input_search(&mut app, "Alpha");

		app.update(crate::ui::app::AppAction::HeaderSearchDebounced(generation));

		let ActiveFilter::HeaderSearch(results) = app.active_filter() else {
			panic!("expected active header search filter");
		};
		assert_eq!(results.label(), "search:Alpha");
		assert_eq!(results.matches.len(), 1);
		assert_eq!(selected_name(&app).as_deref(), Some("AlphaService"));
		assert_eq!(app.view_mode(), VisualizationMode::Search);
	}

	#[test]
	fn applying_header_search_refreshes_open_usage_lens_to_selected_def() {
		let mut app = fixture_app();
		let alpha = def_named(&app, "AlphaService");
		app.focus_usages(alpha);
		let generation = input_search(&mut app, "Beta");

		app.update(crate::ui::app::AppAction::HeaderSearchDebounced(generation));

		let selected = primary_selected_name(&app).expect("selected search match");
		assert_eq!(
			app.usage_lens().map(|focus| focus.label.as_str()),
			Some(selected.as_str())
		);
	}

	#[test]
	fn empty_header_search_preserves_open_usage_lens() {
		let mut app = fixture_app();
		let alpha = def_named(&app, "AlphaService");
		let generation = input_search(&mut app, "Alpha");
		app.update(crate::ui::app::AppAction::HeaderSearchDebounced(generation));
		app.focus_usages(alpha);

		app.update(crate::ui::app::AppAction::Ui(Msg::HeaderSearchReset));

		assert!(matches!(app.active_filter(), ActiveFilter::None));
		assert_eq!(
			app.usage_lens().map(|focus| focus.label.as_str()),
			Some("AlphaService")
		);
	}

	#[test]
	fn stale_header_search_debounce_does_not_apply_old_draft() {
		let mut app = fixture_app();
		let stale_generation = input_search(&mut app, "Alpha");
		app.update(crate::ui::app::AppAction::Ui(Msg::HeaderSearchInput(
			FilterEdit::Clear,
		)));
		let current_generation = input_search(&mut app, "Beta");

		app.update(crate::ui::app::AppAction::HeaderSearchDebounced(
			stale_generation,
		));

		assert!(matches!(app.active_filter(), ActiveFilter::None));
		assert_eq!(
			app.header_search().pending_generation,
			Some(current_generation)
		);
	}

	#[test]
	fn lang_and_kind_filters_constrain_header_search_results() {
		let mut app = fixture_app();
		app.dispatch_shell(ShellAction::SetHeaderSearchFilters {
			langs: vec![Lang::Ts],
			kind_filters: vec![HeaderKindFilter::Kind("function".to_string())],
		});
		let generation = input_search(&mut app, "beta");

		app.update(crate::ui::app::AppAction::HeaderSearchDebounced(generation));

		let ActiveFilter::HeaderSearch(results) = app.active_filter() else {
			panic!("expected active header search filter");
		};
		let names = results
			.matches
			.iter()
			.map(|loc| app.store().symbol_summary(loc).name)
			.collect::<Vec<_>>();
		assert_eq!(results.label(), "search:beta lang:ts kind:function");
		assert_eq!(names, vec!["betaFactory()"]);
	}
}
