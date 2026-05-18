use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;

use crate::ui::events::HeaderSearchFocus;

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
		parts.push(format!(
			"lang:{}",
			langs
				.iter()
				.map(|lang| lang.tag())
				.collect::<Vec<_>>()
				.join(",")
		));
	}
	if !kind_filters.is_empty() {
		parts.push(format!(
			"kind:{}",
			kind_filters
				.iter()
				.map(HeaderKindFilter::label)
				.collect::<Vec<_>>()
				.join(",")
		));
	}
	if parts.is_empty() {
		"<all>".to_string()
	} else {
		parts.join(" ")
	}
}
