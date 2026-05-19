use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::core::moniker::{Moniker, Segment};
use rustc_hash::{FxHashMap, FxHashSet};

use super::index::{DefLocation, RefLocation, SessionIndex};

#[derive(Clone, Debug, Default)]
pub(crate) struct LinkageIndex {
	stats: LinkageStats,
	refs_by_source: FxHashMap<Moniker, Vec<RefLocation>>,
	refs_by_target_key: FxHashMap<LinkKey, Vec<RefLocation>>,
	refs_by_target_projectless_key: FxHashMap<LinkKey, Vec<RefLocation>>,
	refs_by_target_ancestor: FxHashMap<PathKey, Vec<RefLocation>>,
	refs_by_target_projectless_ancestor: FxHashMap<PathKey, Vec<RefLocation>>,
	refs_by_callable_name: FxHashMap<Vec<u8>, Vec<RefLocation>>,
	resolved_defs_by_ref: FxHashMap<RefLocation, Vec<DefLocation>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LinkageStats {
	pub(crate) resolved_refs: usize,
	pub(crate) unresolved_refs: usize,
	pub(crate) ambiguous_refs: usize,
}

impl LinkageIndex {
	pub(crate) fn build(index: &SessionIndex) -> Self {
		let mut linkage = Self::default();
		let defs_by_key = defs_by_link_key(index);
		for (file_idx, file) in index.files.iter().enumerate() {
			for (ref_idx, reference) in file.graph.refs().enumerate() {
				let loc = RefLocation {
					file: file_idx,
					reference: ref_idx,
				};
				let source = file.graph.def_at(reference.source).moniker.clone();
				linkage.refs_by_source.entry(source).or_default().push(loc);
				linkage.index_target_reference(&reference.target, loc);
				let resolved = resolve_reference(index, &defs_by_key, reference);
				if !resolved.is_empty() {
					linkage.stats.resolved_refs += 1;
					if resolved.len() > 1 {
						linkage.stats.ambiguous_refs += 1;
					}
					linkage.resolved_defs_by_ref.insert(loc, resolved);
				} else {
					linkage.stats.unresolved_refs += 1;
				}
			}
		}
		linkage
	}

	pub(crate) fn stats(&self) -> &LinkageStats {
		&self.stats
	}

	pub(crate) fn outgoing_refs(&self, source: &Moniker) -> &[RefLocation] {
		self.refs_by_source.get(source).map_or(&[], Vec::as_slice)
	}

	pub(crate) fn incoming_refs(&self, target: &Moniker, index: &SessionIndex) -> Vec<RefLocation> {
		let mut seen = FxHashSet::default();
		let mut out = Vec::new();
		self.collect_matching_refs(
			self.refs_by_target_key.get(&LinkKey::from_moniker(target)),
			target,
			index,
			&mut seen,
			&mut out,
		);
		self.collect_matching_refs(
			self.refs_by_target_projectless_key
				.get(&LinkKey::projectless(target)),
			target,
			index,
			&mut seen,
			&mut out,
		);
		self.collect_matching_refs(
			self.refs_by_target_ancestor
				.get(&PathKey::from_moniker(target)),
			target,
			index,
			&mut seen,
			&mut out,
		);
		self.collect_matching_refs(
			self.refs_by_target_projectless_ancestor
				.get(&PathKey::projectless(target)),
			target,
			index,
			&mut seen,
			&mut out,
		);
		if is_callable_moniker(target) {
			self.collect_matching_refs(
				self.refs_by_callable_name
					.get(last_bare_name(target).expect("callable moniker has a last segment")),
				target,
				index,
				&mut seen,
				&mut out,
			);
		}
		out.sort_by_key(|loc| (index.files[loc.file].rel_path.clone(), loc.reference));
		out
	}

	#[allow(dead_code)]
	pub(crate) fn resolved_defs(&self, loc: &RefLocation) -> &[DefLocation] {
		self.resolved_defs_by_ref
			.get(loc)
			.map_or(&[], Vec::as_slice)
	}

	fn index_target_reference(&mut self, target: &Moniker, loc: RefLocation) {
		self.refs_by_target_key
			.entry(LinkKey::from_moniker(target))
			.or_default()
			.push(loc);
		self.refs_by_target_projectless_key
			.entry(LinkKey::projectless(target))
			.or_default()
			.push(loc);
		for key in PathKey::ancestors(target, true) {
			self.refs_by_target_ancestor
				.entry(key)
				.or_default()
				.push(loc);
		}
		for key in PathKey::ancestors(target, false) {
			self.refs_by_target_projectless_ancestor
				.entry(key)
				.or_default()
				.push(loc);
		}
		if let Some(name) = last_bare_name(target) {
			self.refs_by_callable_name
				.entry(name.to_vec())
				.or_default()
				.push(loc);
		}
	}

	fn collect_matching_refs(
		&self,
		candidates: Option<&Vec<RefLocation>>,
		target: &Moniker,
		index: &SessionIndex,
		seen: &mut FxHashSet<RefLocation>,
		out: &mut Vec<RefLocation>,
	) {
		let Some(candidates) = candidates else {
			return;
		};
		for loc in candidates {
			if seen.insert(*loc) {
				let reference = index.reference(loc);
				if usage_target_matches(target, &reference.target) {
					out.push(*loc);
				}
			}
		}
	}
}

fn defs_by_link_key(index: &SessionIndex) -> FxHashMap<LinkKey, Vec<DefLocation>> {
	let mut defs = FxHashMap::default();
	for (file_idx, file) in index.files.iter().enumerate() {
		for (def_idx, def) in file.graph.defs().enumerate() {
			defs.entry(LinkKey::from_moniker(&def.moniker))
				.or_insert_with(Vec::new)
				.push(DefLocation {
					file: file_idx,
					def: def_idx,
				});
		}
	}
	defs
}

fn resolve_reference(
	index: &SessionIndex,
	defs_by_key: &FxHashMap<LinkKey, Vec<DefLocation>>,
	reference: &RefRecord,
) -> Vec<DefLocation> {
	let Some(candidates) = defs_by_key.get(&LinkKey::from_moniker(&reference.target)) else {
		return Vec::new();
	};
	candidates
		.iter()
		.copied()
		.filter(|loc| reference.target.bind_match(&index.def(loc).moniker))
		.collect()
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct LinkKey {
	project: Option<Vec<u8>>,
	parents: Vec<(Vec<u8>, Vec<u8>)>,
	bare_last_name: Vec<u8>,
}

impl LinkKey {
	fn from_moniker(moniker: &Moniker) -> Self {
		Self::new(moniker, true)
	}

	fn projectless(moniker: &Moniker) -> Self {
		Self::new(moniker, false)
	}

	fn new(moniker: &Moniker, include_project: bool) -> Self {
		let view = moniker.as_view();
		let segments: Vec<_> = view.segments().collect();
		let parents = segments
			.iter()
			.take(segments.len().saturating_sub(1))
			.map(|segment| (segment.kind.to_vec(), segment.name.to_vec()))
			.collect();
		let bare_last_name = segments
			.last()
			.map(|segment| bare_callable_name(segment.name).to_vec())
			.unwrap_or_default();
		Self {
			project: include_project.then(|| view.project().to_vec()),
			parents,
			bare_last_name,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PathKey {
	project: Option<Vec<u8>>,
	segments: Vec<(Vec<u8>, Vec<u8>)>,
}

impl PathKey {
	fn from_moniker(moniker: &Moniker) -> Self {
		Self::new(moniker, true)
	}

	fn projectless(moniker: &Moniker) -> Self {
		Self::new(moniker, false)
	}

	fn ancestors(moniker: &Moniker, include_project: bool) -> Vec<Self> {
		let view = moniker.as_view();
		let segments: Vec<_> = view.segments().collect();
		(1..segments.len())
			.map(|len| Self {
				project: include_project.then(|| view.project().to_vec()),
				segments: segments[..len]
					.iter()
					.map(|segment| (segment.kind.to_vec(), segment.name.to_vec()))
					.collect(),
			})
			.collect()
	}

	fn new(moniker: &Moniker, include_project: bool) -> Self {
		let view = moniker.as_view();
		Self {
			project: include_project.then(|| view.project().to_vec()),
			segments: view
				.segments()
				.map(|segment| (segment.kind.to_vec(), segment.name.to_vec()))
				.collect(),
		}
	}
}

fn usage_target_matches(selected: &Moniker, reference_target: &Moniker) -> bool {
	selected.bind_match(reference_target)
		|| selected.is_ancestor_of(reference_target)
		|| moniker_matches_without_project(selected, reference_target)
		|| moniker_is_ancestor_without_project(selected, reference_target)
		|| callable_last_segment_matches(selected, reference_target)
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

fn callable_last_segment_matches(selected: &Moniker, reference_target: &Moniker) -> bool {
	let Some(selected_segment) = selected.as_view().segments().last() else {
		return false;
	};
	let Some(target_segment) = reference_target.as_view().segments().last() else {
		return false;
	};
	if !is_callable_def_kind(selected_segment.kind) {
		return false;
	}
	bare_callable_name(selected_segment.name) == bare_callable_name(target_segment.name)
}

fn last_bare_name(moniker: &Moniker) -> Option<&[u8]> {
	moniker
		.as_view()
		.segments()
		.last()
		.map(|segment| bare_callable_name(segment.name))
}

fn is_callable_def_kind(kind: &[u8]) -> bool {
	matches!(kind, b"method" | b"function" | b"func" | b"constructor")
}

fn is_callable_moniker(moniker: &Moniker) -> bool {
	moniker
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| is_callable_def_kind(segment.kind))
}

fn bare_callable_name(name: &[u8]) -> &[u8] {
	name.iter()
		.position(|b| *b == b'(')
		.map_or(name, |idx| &name[..idx])
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use super::*;
	use crate::workspace::{SessionOptions, symbols::last_name};

	fn write(root: &Path, rel: &str, body: &str) {
		let p = root.join(rel);
		if let Some(parent) = p.parent() {
			std::fs::create_dir_all(parent).unwrap();
		}
		std::fs::write(p, body).unwrap();
	}

	#[test]
	fn incoming_refs_resolve_import_placeholders_to_typed_defs() {
		let tmp = tempfile::tempdir().unwrap();
		write(tmp.path(), "src/lib.ts", "export class Lib {}\n");
		write(
			tmp.path(),
			"src/app.ts",
			"import { Lib } from './lib'; export const value = Lib;\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		let linkage = LinkageIndex::build(&index);
		let lib = index
			.defs_by_name
			.get("Lib")
			.and_then(|locs| {
				locs.iter()
					.find(|loc| last_name(&index.def(loc).moniker) == "Lib")
			})
			.copied()
			.expect("Lib def");

		let refs = linkage.incoming_refs(&index.def(&lib).moniker, &index);

		assert!(
			refs.iter().any(|loc| index
				.reference(loc)
				.target
				.bind_match(&index.def(&lib).moniker)),
			"expected import placeholder to resolve to typed Lib def"
		);
	}

	#[test]
	fn incoming_refs_include_descendant_targets_for_changed_parent_symbols() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"src/lib.ts",
			"export class Lib { run() { return 1; } }\n",
		);
		write(
			tmp.path(),
			"src/app.ts",
			"import { Lib } from './lib'; export const value = new Lib().run();\n",
		);
		let index = SessionIndex::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();
		let linkage = LinkageIndex::build(&index);
		let lib = index
			.defs_by_name
			.get("Lib")
			.and_then(|locs| {
				locs.iter()
					.find(|loc| last_name(&index.def(loc).moniker) == "Lib")
			})
			.copied()
			.expect("Lib def");

		let refs = linkage.incoming_refs(&index.def(&lib).moniker, &index);

		assert!(!refs.is_empty());
	}
}
