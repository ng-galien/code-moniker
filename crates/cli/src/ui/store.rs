use std::path::Path;

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::{Moniker, Segment};
use code_moniker_core::lang::Lang;

use crate::inspect::{
	CheckSummary, DefLocation, IndexedFile, RefLocation, SessionIndex, SessionOptions, SessionStats,
};

use super::filter::NavFilter;
use super::kinds::{definition_kind_order, is_navigable_definition};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct UsageFocus {
	pub(super) target: Moniker,
	pub(super) label: String,
	pub(super) refs: Vec<RefLocation>,
	pub(super) contexts: Vec<DefLocation>,
}

pub(super) trait IndexStore {
	fn root(&self) -> &str;
	fn stats(&self) -> &SessionStats;
	fn file_count(&self) -> usize;
	fn file(&self, file_idx: usize) -> &IndexedFile;
	fn def(&self, loc: &DefLocation) -> &DefRecord;
	fn reference(&self, loc: &RefLocation) -> &RefRecord;
	fn all_navigable_defs(&self, filter: Option<&NavFilter>) -> Vec<DefLocation>;
	fn root_defs(&self, file_idx: usize) -> Vec<DefLocation>;
	fn child_defs(&self, parent: &DefLocation) -> Vec<DefLocation>;
	fn children_by_parent(&self, parent: &Moniker) -> &[DefLocation];
	fn outgoing_refs(&self, moniker: &Moniker) -> &[RefLocation];
	fn incoming_refs(&self, moniker: &Moniker) -> &[RefLocation];
	fn usage_focus(&self, loc: DefLocation) -> UsageFocus;
	fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary>;
}

pub(super) struct MemoryIndexStore {
	index: SessionIndex,
}

impl MemoryIndexStore {
	pub(super) fn load(opts: &SessionOptions) -> anyhow::Result<Self> {
		Ok(Self::new(SessionIndex::load(opts)?))
	}

	pub(super) fn new(index: SessionIndex) -> Self {
		Self { index }
	}
}

impl IndexStore for MemoryIndexStore {
	fn root(&self) -> &str {
		&self.index.root
	}

	fn stats(&self) -> &SessionStats {
		&self.index.stats
	}

	fn file_count(&self) -> usize {
		self.index.files.len()
	}

	fn file(&self, file_idx: usize) -> &IndexedFile {
		&self.index.files[file_idx]
	}

	fn def(&self, loc: &DefLocation) -> &DefRecord {
		self.index.def(loc)
	}

	fn reference(&self, loc: &RefLocation) -> &RefRecord {
		self.index.reference(loc)
	}

	fn all_navigable_defs(&self, filter: Option<&NavFilter>) -> Vec<DefLocation> {
		let mut out: Vec<DefLocation> = self
			.index
			.files
			.iter()
			.enumerate()
			.flat_map(|(file_idx, file)| {
				file.graph
					.defs()
					.enumerate()
					.map(move |(def_idx, _)| DefLocation {
						file: file_idx,
						def: def_idx,
					})
			})
			.filter(|loc| {
				let def = self.def(loc);
				is_navigable_def(self.file(loc.file).lang, def)
					&& filter.is_none_or(|filter| {
						filter.matches(&def_kind(def), &last_name(&def.moniker))
					})
			})
			.collect();
		out.sort_by(|a, b| self.def(a).moniker.cmp(&self.def(b).moniker));
		out
	}

	fn root_defs(&self, file_idx: usize) -> Vec<DefLocation> {
		let mut locs: Vec<DefLocation> = self.index.files[file_idx]
			.graph
			.defs()
			.enumerate()
			.filter(|(_, def)| def.parent.is_none())
			.map(|(def_idx, _)| DefLocation {
				file: file_idx,
				def: def_idx,
			})
			.collect();
		self.sort_defs_by_position(&mut locs);
		locs
	}

	fn child_defs(&self, parent: &DefLocation) -> Vec<DefLocation> {
		let mut locs: Vec<DefLocation> = self
			.index
			.children_by_parent
			.get(&self.def(parent).moniker)
			.into_iter()
			.flat_map(|children| children.iter().copied())
			.filter(|loc| loc.file == parent.file)
			.collect();
		self.sort_defs_by_position(&mut locs);
		locs
	}

	fn children_by_parent(&self, parent: &Moniker) -> &[DefLocation] {
		self.index
			.children_by_parent
			.get(parent)
			.map_or(&[], Vec::as_slice)
	}

	fn outgoing_refs(&self, moniker: &Moniker) -> &[RefLocation] {
		self.index.outgoing_refs(moniker)
	}

	fn incoming_refs(&self, moniker: &Moniker) -> &[RefLocation] {
		self.index.incoming_refs(moniker)
	}

	fn usage_focus(&self, loc: DefLocation) -> UsageFocus {
		let target = self.def(&loc).moniker.clone();
		let label = last_name(&target);
		let refs = self.refs_matching_target(&target);
		let contexts = self.usage_contexts(&refs);
		UsageFocus {
			target,
			label,
			refs,
			contexts,
		}
	}

	fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary> {
		self.index.check_summary(rules, profile, scheme)
	}
}

impl MemoryIndexStore {
	fn sort_defs_by_position(&self, locs: &mut [DefLocation]) {
		locs.sort_by(|a, b| {
			let left = self.def(a);
			let right = self.def(b);
			left.position
				.map(|(start, _)| start)
				.cmp(&right.position.map(|(start, _)| start))
				.then_with(|| {
					definition_kind_order(self.file(a.file).lang, &def_kind(left)).cmp(
						&definition_kind_order(self.file(b.file).lang, &def_kind(right)),
					)
				})
				.then_with(|| last_name(&left.moniker).cmp(&last_name(&right.moniker)))
		});
	}

	fn refs_matching_target(&self, target: &Moniker) -> Vec<RefLocation> {
		let mut refs = Vec::new();
		for (file_idx, file) in self.index.files.iter().enumerate() {
			for (ref_idx, reference) in file.graph.refs().enumerate() {
				if usage_target_matches(target, &reference.target) {
					refs.push(RefLocation {
						file: file_idx,
						reference: ref_idx,
					});
				}
			}
		}
		refs
	}

	fn usage_contexts(&self, refs: &[RefLocation]) -> Vec<DefLocation> {
		let mut out = Vec::new();
		for loc in refs {
			for context in self.nav_contexts_for_ref(loc) {
				if !out.contains(&context) {
					out.push(context);
				}
			}
		}
		out.sort_by(|a, b| {
			self.file(a.file)
				.rel_path
				.cmp(&self.file(b.file).rel_path)
				.then_with(|| self.def(a).moniker.cmp(&self.def(b).moniker))
		});
		out
	}

	fn nav_contexts_for_ref(&self, loc: &RefLocation) -> Vec<DefLocation> {
		let reference = self.reference(loc);
		let source = DefLocation {
			file: loc.file,
			def: reference.source,
		};
		if is_navigable_def(self.file(source.file).lang, self.def(&source)) {
			return vec![source];
		}
		let source_moniker = self.def(&source).moniker.clone();
		self.children_by_parent(&source_moniker)
			.iter()
			.copied()
			.filter(|child| {
				child.file == loc.file
					&& is_navigable_def(self.file(child.file).lang, self.def(child))
			})
			.collect()
	}
}

pub(super) fn is_navigable_def(lang: Lang, def: &DefRecord) -> bool {
	is_navigable_definition(lang, &def_kind(def))
}

pub(super) fn def_kind(def: &DefRecord) -> String {
	std::str::from_utf8(&def.kind).unwrap_or("?").to_string()
}

pub(super) fn ref_kind(reference: &RefRecord) -> String {
	std::str::from_utf8(&reference.kind)
		.unwrap_or("?")
		.to_string()
}

pub(super) fn last_name(moniker: &Moniker) -> String {
	moniker
		.as_view()
		.segments()
		.last()
		.and_then(|s| std::str::from_utf8(s.name).ok())
		.unwrap_or(".")
		.to_string()
}

pub(super) fn compact_moniker(moniker: &Moniker) -> String {
	let view = moniker.as_view();
	let project = std::str::from_utf8(view.project()).unwrap_or(".");
	let mut out = String::from(project);
	for seg in view.segments() {
		let kind = std::str::from_utf8(seg.kind).unwrap_or("?");
		let name = std::str::from_utf8(seg.name).unwrap_or("?");
		out.push('/');
		out.push_str(kind);
		out.push(':');
		out.push_str(name);
	}
	out
}

fn usage_target_matches(selected: &Moniker, reference_target: &Moniker) -> bool {
	selected.bind_match(reference_target)
		|| selected.is_ancestor_of(reference_target)
		|| moniker_matches_without_project(selected, reference_target)
		|| moniker_is_ancestor_without_project(selected, reference_target)
}

fn moniker_matches_without_project(left: &Moniker, right: &Moniker) -> bool {
	let left_segments: Vec<_> = left.as_view().segments().collect();
	let right_segments: Vec<_> = right.as_view().segments().collect();
	if left_segments.len() != right_segments.len() || left_segments.is_empty() {
		return false;
	}
	let last_idx = left_segments.len() - 1;
	left_segments[..last_idx] == right_segments[..last_idx]
		&& segment_names_match(left_segments[last_idx], right_segments[last_idx])
}

fn moniker_is_ancestor_without_project(parent: &Moniker, child: &Moniker) -> bool {
	let parent_segments: Vec<_> = parent.as_view().segments().collect();
	let child_segments: Vec<_> = child.as_view().segments().collect();
	if parent_segments.is_empty() || parent_segments.len() >= child_segments.len() {
		return false;
	}
	child_segments.starts_with(&parent_segments)
}

fn segment_names_match(left: Segment<'_>, right: Segment<'_>) -> bool {
	left.name == right.name || bare_callable_name(left.name) == bare_callable_name(right.name)
}

fn bare_callable_name(name: &[u8]) -> &[u8] {
	name.iter()
		.position(|b| *b == b'(')
		.map_or(name, |idx| &name[..idx])
}
