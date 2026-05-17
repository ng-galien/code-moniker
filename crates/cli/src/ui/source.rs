use crate::workspace::DefLocation;

use super::App;
use crate::workspace::IndexStore;

#[derive(Clone, Debug)]
pub(super) struct SourceLineVm {
	pub(super) number: u32,
	pub(super) number_width: usize,
	pub(super) text: String,
	pub(super) active: bool,
}

pub(super) fn source_snippet(app: &App, loc: &DefLocation, context: u32) -> Vec<SourceLineVm> {
	let snippet = app.store().source_snippet(loc, context);
	let width = snippet
		.iter()
		.map(|line| line.number.to_string().len())
		.max()
		.unwrap_or(4)
		.max(4);
	snippet
		.into_iter()
		.map(|line| SourceLineVm {
			number: line.number,
			number_width: width,
			text: line.text,
			active: line.active,
		})
		.collect()
}
