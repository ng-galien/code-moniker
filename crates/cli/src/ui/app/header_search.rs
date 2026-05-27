use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;

use crate::ui::app::{
	App, apply_navigation, dispatch_and_apply, refresh_results, select_def, sync_contextual_view,
};
use crate::ui::events::{HeaderSearchFocus, UiMode};
use crate::ui::explorer::{
	HeaderSearchResults, header_search_options,
	header_search_results as explorer_header_search_results,
};
use crate::ui::store::navigation::{NavigationAction, NavigationScope};
use crate::ui::workspace_read::{self, LocalWorkspaceFacade};
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

struct HeaderSelectorDecision {
	shell: Vec<ShellAction>,
	status: Option<String>,
	apply_search: bool,
}

impl HeaderSelectorDecision {
	fn status(status: impl Into<String>) -> Self {
		Self {
			shell: Vec::new(),
			status: Some(status.into()),
			apply_search: false,
		}
	}

	fn shell(shell: ShellAction, status: impl Into<String>) -> Self {
		Self {
			shell: vec![shell],
			status: Some(status.into()),
			apply_search: false,
		}
	}

	fn apply_search() -> Self {
		Self {
			shell: Vec::new(),
			status: None,
			apply_search: true,
		}
	}
}

fn cycle_header_search_selector_decision(
	mode: UiMode,
	header: &HeaderSearchState,
	direction: i8,
) -> HeaderSelectorDecision {
	match header_focus(mode) {
		HeaderSearchFocus::Text => {
			HeaderSelectorDecision::status("type text or press Tab to edit language")
		}
		HeaderSearchFocus::Lang if !header.combo_open => HeaderSelectorDecision::status(
			"press Enter to open the selector, Space toggles an option",
		),
		HeaderSearchFocus::Lang => {
			let cursor = cycle_index(
				header.lang_cursor,
				header.available_langs.len() + 1,
				direction,
			);
			HeaderSelectorDecision::shell(
				ShellAction::SetHeaderSearchCursor {
					focus: HeaderSearchFocus::Lang,
					cursor,
				},
				format!(
					"language option: {}",
					lang_selector_option_label(&header.langs, &header.available_langs, cursor)
				),
			)
		}
		HeaderSearchFocus::Kind => {
			let cursor = cycle_index(
				header.kind_cursor,
				header.available_kind_filters.len() + 1,
				direction,
			);
			HeaderSelectorDecision::shell(
				ShellAction::SetHeaderSearchCursor {
					focus: HeaderSearchFocus::Kind,
					cursor,
				},
				format!(
					"kind option: {}",
					kind_selector_option_label(
						&header.kind_filters,
						&header.available_kind_filters,
						cursor
					)
				),
			)
		}
	}
}

fn toggle_header_search_selection_decision(
	mode: UiMode,
	header: &HeaderSearchState,
) -> HeaderSelectorDecision {
	match header_focus(mode) {
		HeaderSearchFocus::Text => HeaderSelectorDecision::apply_search(),
		HeaderSearchFocus::Lang if !header.combo_open => HeaderSelectorDecision::status(
			"press Enter to open the selector, Space toggles an option",
		),
		HeaderSearchFocus::Lang => toggle_header_lang_selection(header),
		HeaderSearchFocus::Kind if !header.combo_open => HeaderSelectorDecision::status(
			"press Enter to open the selector, Space toggles an option",
		),
		HeaderSearchFocus::Kind => toggle_header_kind_selection(header),
	}
}

fn toggle_header_lang_selection(header: &HeaderSearchState) -> HeaderSelectorDecision {
	let cursor = header.lang_cursor.min(header.available_langs.len());
	let mut langs = header.langs.clone();
	if cursor == 0 {
		langs.clear();
	} else {
		toggle_value(&mut langs, header.available_langs[cursor - 1]);
	}
	HeaderSelectorDecision::shell(
		ShellAction::SetHeaderSearchFilters {
			langs: langs.clone(),
			kind_filters: header.kind_filters.clone(),
		},
		format!("language filter: {}", lang_filter_summary(&langs)),
	)
}

fn toggle_header_kind_selection(header: &HeaderSearchState) -> HeaderSelectorDecision {
	let cursor = header.kind_cursor.min(header.available_kind_filters.len());
	let mut filters = header.kind_filters.clone();
	if cursor == 0 {
		filters.clear();
	} else {
		toggle_value(
			&mut filters,
			header.available_kind_filters[cursor - 1].clone(),
		);
	}
	HeaderSelectorDecision::shell(
		ShellAction::SetHeaderSearchFilters {
			langs: header.langs.clone(),
			kind_filters: filters.clone(),
		},
		format!("kind filter: {}", kind_filter_summary(&filters)),
	)
}

fn header_focus(mode: UiMode) -> HeaderSearchFocus {
	match mode {
		UiMode::HeaderSearch(focus) => focus,
		UiMode::Normal => HeaderSearchFocus::Text,
	}
}

fn decide_apply_header_search(
	header: &HeaderSearchState,
	store: &LocalWorkspaceFacade,
	generation: Option<u64>,
	return_focus: bool,
) -> HeaderSearchDecision {
	if generation.is_some() && generation != header.pending_generation {
		return HeaderSearchDecision::stale();
	}
	if !header.has_filter() {
		let visible_defs = workspace_read::all_navigable_defs(store);
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
			workspace_read::stats(store).defs
		)
	} else {
		format!(
			"search: {} ({}/{})",
			results.label(),
			match_count,
			workspace_read::stats(store).defs
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
			crate::ui::app::header_search(self),
			crate::ui::app::store(self),
			generation,
			return_focus,
		);
		for action in decision.shell {
			crate::ui::app::dispatch_shell(self, action);
		}
		for action in decision.navigation {
			apply_navigation(self, action);
		}
		if let Some(loc) = decision.select {
			select_def(self, loc);
		}
		if decision.sync_contextual_view {
			sync_contextual_view(self);
		}
		if let Some(status) = decision.status {
			crate::ui::app::set_status(self, status);
		}
	}

	pub(in crate::ui) fn header_search_results(
		&self,
		text: &str,
		langs: &[Lang],
		kind_filters: &[HeaderKindFilter],
	) -> HeaderSearchResults {
		explorer_header_search_results(crate::ui::app::store(self), text, langs, kind_filters)
	}

	pub(in crate::ui) fn cycle_header_search_selector(&mut self, direction: i8) {
		let decision = cycle_header_search_selector_decision(
			crate::ui::app::mode(self),
			crate::ui::app::header_search(self),
			direction,
		);
		self.apply_header_selector_decision(decision);
	}

	pub(in crate::ui) fn toggle_header_search_selection(&mut self) {
		let decision = toggle_header_search_selection_decision(
			crate::ui::app::mode(self),
			crate::ui::app::header_search(self),
		);
		if decision.apply_search {
			self.apply_header_search(None, true);
		}
		self.apply_header_selector_decision(decision);
	}

	pub(in crate::ui) fn refresh_header_search_options(&mut self) {
		let options = header_search_options(
			crate::ui::app::store(self),
			crate::ui::app::header_search(self),
		);
		dispatch_and_apply(
			self,
			&AppAction::Shell(ShellAction::SetHeaderSearchOptions {
				langs: options.langs,
				kind_filters: options.kind_filters,
				available_langs: options.available_langs,
				available_kind_filters: options.available_kind_filters,
				lang_cursor: options.lang_cursor,
				kind_cursor: options.kind_cursor,
			}),
		);
	}

	fn apply_header_selector_decision(&mut self, decision: HeaderSelectorDecision) {
		for action in decision.shell {
			crate::ui::app::dispatch_shell(self, action);
		}
		if let Some(status) = decision.status {
			crate::ui::app::set_status(self, status);
		}
	}

	pub(in crate::ui) fn clear_filter(&mut self) {
		self.clear_filter_with_focus(true);
	}

	pub(in crate::ui) fn clear_filter_with_focus(&mut self, return_focus: bool) {
		crate::ui::app::dispatch_shell(self, ShellAction::ClearFilter { return_focus });
		refresh_results(self, true);
		sync_contextual_view(self);
		crate::ui::app::set_status(self, "filter cleared");
	}
}
