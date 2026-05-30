use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;

use crate::ui::app::{HeaderKindFilter, HeaderSearchState, header_search_label};
use crate::ui::workspace_read::{self, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::SymbolId;
type DefLocation = SymbolId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct HeaderSearchResults {
	pub(in crate::ui) text: String,
	pub(in crate::ui) langs: Vec<Lang>,
	pub(in crate::ui) kind_filters: Vec<HeaderKindFilter>,
	pub(in crate::ui) matches: Vec<DefLocation>,
}

impl HeaderSearchResults {
	pub(in crate::ui) fn label(&self) -> String {
		header_search_label(&self.text, &self.langs, &self.kind_filters)
	}
}

pub(in crate::ui) struct HeaderSearchOptions {
	pub(in crate::ui) langs: Vec<Lang>,
	pub(in crate::ui) kind_filters: Vec<HeaderKindFilter>,
	pub(in crate::ui) available_langs: Vec<Lang>,
	pub(in crate::ui) available_kind_filters: Vec<HeaderKindFilter>,
	pub(in crate::ui) lang_cursor: usize,
	pub(in crate::ui) kind_cursor: usize,
}

pub(in crate::ui) fn header_search_results(
	store: &LocalWorkspaceRegistry,
	text: &str,
	langs: &[Lang],
	kind_filters: &[HeaderKindFilter],
) -> HeaderSearchResults {
	let raw = text.trim().to_string();
	let (kind_names, shapes) = split_kind_filters(kind_filters);
	let matches = if raw.is_empty() {
		workspace_read::navigable_defs_filtered(store, langs, &kind_names, &shapes)
	} else {
		workspace_read::search_symbols_filtered(store, &raw, 500, langs, &kind_names, &shapes)
			.into_iter()
			.map(|hit| hit.loc)
			.collect()
	};
	HeaderSearchResults {
		text: raw,
		langs: langs.to_vec(),
		kind_filters: kind_filters.to_vec(),
		matches,
	}
}

pub(in crate::ui) fn header_search_options(
	store: &LocalWorkspaceRegistry,
	state: &HeaderSearchState,
) -> HeaderSearchOptions {
	let available_langs = workspace_read::available_langs(store);
	let langs = normalize_langs(state.langs.clone(), &available_langs);
	let available_kind_filters = compute_kind_filter_options(store, &langs);
	let kind_filters =
		normalize_kind_filters(state.kind_filters.clone(), &langs, &available_kind_filters);
	HeaderSearchOptions {
		lang_cursor: state.lang_cursor.min(available_langs.len()),
		kind_cursor: state.kind_cursor.min(available_kind_filters.len()),
		langs,
		kind_filters,
		available_langs,
		available_kind_filters,
	}
}

fn compute_kind_filter_options(
	store: &LocalWorkspaceRegistry,
	langs: &[Lang],
) -> Vec<HeaderKindFilter> {
	if langs.len() == 1 {
		return workspace_read::available_kinds_for_lang(store, langs[0])
			.into_iter()
			.map(HeaderKindFilter::Kind)
			.collect();
	}
	workspace_read::available_shapes(store, langs)
		.into_iter()
		.map(HeaderKindFilter::Shape)
		.collect()
}

fn normalize_langs(langs: Vec<Lang>, available: &[Lang]) -> Vec<Lang> {
	Lang::ALL
		.iter()
		.copied()
		.filter(|lang| available.contains(lang) && langs.contains(lang))
		.collect()
}

fn normalize_kind_filters(
	filters: Vec<HeaderKindFilter>,
	langs: &[Lang],
	available: &[HeaderKindFilter],
) -> Vec<HeaderKindFilter> {
	let mut normalized = Vec::new();
	if langs.len() == 1 {
		for filter in filters {
			match filter {
				HeaderKindFilter::Kind(kind) => {
					push_unique(&mut normalized, HeaderKindFilter::Kind(kind));
				}
				HeaderKindFilter::Shape(shape) => {
					let before = normalized.len();
					for option in available {
						if let HeaderKindFilter::Kind(kind) = option
							&& shape_of(kind.as_bytes()) == Some(shape)
						{
							push_unique(&mut normalized, HeaderKindFilter::Kind(kind.clone()));
						}
					}
					if normalized.len() == before {
						push_unique(&mut normalized, HeaderKindFilter::Shape(shape));
					}
				}
			}
		}
	} else {
		for filter in filters {
			let shape = match filter {
				HeaderKindFilter::Kind(kind) => shape_of(kind.as_bytes()),
				HeaderKindFilter::Shape(shape) => Some(shape),
			};
			if let Some(shape) = shape {
				push_unique(&mut normalized, HeaderKindFilter::Shape(shape));
			}
		}
	}
	normalized
}

fn split_kind_filters(filters: &[HeaderKindFilter]) -> (Vec<String>, Vec<Shape>) {
	let mut kinds = Vec::new();
	let mut shapes = Vec::new();
	for filter in filters {
		match filter {
			HeaderKindFilter::Kind(kind) => push_unique(&mut kinds, kind.clone()),
			HeaderKindFilter::Shape(shape) => push_unique(&mut shapes, *shape),
		}
	}
	(kinds, shapes)
}

fn push_unique<T: Eq>(values: &mut Vec<T>, value: T) {
	if !values.contains(&value) {
		values.push(value);
	}
}
