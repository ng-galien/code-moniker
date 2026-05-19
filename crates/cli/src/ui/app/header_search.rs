use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;

use crate::ui::app::App;
use crate::ui::events::{HeaderSearchFocus, UiMode};
use crate::ui::explorer::{
	HeaderSearchResults, header_search_options,
	header_search_results as explorer_header_search_results,
};
use crate::ui::store::navigation::NavigationAction;
use crate::workspace::IndexStore;

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

impl App {
	pub(in crate::ui) fn apply_header_search(
		&mut self,
		generation: Option<u64>,
		return_focus: bool,
	) {
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
		self.dispatch_navigation(NavigationAction::ClearUsageLens);
		self.refresh_results(true);
		self.sync_contextual_view();
		self.set_status("filter cleared");
	}
}
