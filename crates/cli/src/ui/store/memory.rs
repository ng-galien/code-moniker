use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::{Moniker, Segment};
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::inspect::{
	CheckSummary, DefLocation, IndexedFile, RefLocation, SessionIndex, SessionOptions, SessionStats,
};
use crate::sources;
use crate::ui::change::{ChangeEntry, ChangeFile, ChangeIndex, ChangeRoot, ChangeScan};
use crate::ui::filter::NavFilter;
use crate::ui::kinds::{definition_kind_order, is_navigable_definition};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct UsageFocus {
	pub(in crate::ui) target: Moniker,
	pub(in crate::ui) label: String,
	pub(in crate::ui) refs: Vec<RefLocation>,
	pub(in crate::ui) contexts: Vec<DefLocation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct SearchHit {
	pub(in crate::ui) loc: DefLocation,
	pub(in crate::ui) score: u32,
	pub(in crate::ui) reason: String,
}

pub(in crate::ui) trait IndexStore {
	fn root(&self) -> &str;
	fn stats(&self) -> &SessionStats;
	fn file_count(&self) -> usize;
	fn file(&self, file_idx: usize) -> &IndexedFile;
	fn def(&self, loc: &DefLocation) -> &DefRecord;
	fn reference(&self, loc: &RefLocation) -> &RefRecord;
	fn all_navigable_defs(&self, filter: Option<&NavFilter>) -> Vec<DefLocation>;
	fn root_defs(&self, file_idx: usize) -> Vec<DefLocation>;
	fn child_defs(&self, parent: &DefLocation) -> Vec<DefLocation>;
	fn compare_defs_for_navigation(&self, left: &DefLocation, right: &DefLocation) -> Ordering;
	fn children_by_parent(&self, parent: &Moniker) -> &[DefLocation];
	fn search_symbols(&self, query: &str, limit: usize) -> Vec<SearchHit>;
	fn change_index(&self) -> &ChangeIndex;
	fn changed_defs(&self) -> Vec<DefLocation>;
	fn change_for_def(&self, loc: &DefLocation) -> Option<&ChangeEntry>;
	fn change_count_for_file(&self, file_idx: usize) -> usize;
	fn change_usage_refs(&self, change: &ChangeEntry) -> &[RefLocation];
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct StoreWatchRoot {
	pub(in crate::ui) path: PathBuf,
	pub(in crate::ui) git_root: Option<PathBuf>,
	pub(in crate::ui) ignored_paths: Vec<PathBuf>,
}

pub(in crate::ui) struct MemoryIndexStore {
	opts: SessionOptions,
	index: Arc<SessionIndex>,
	search_docs: Arc<Vec<SearchDoc>>,
	change_index: ChangeIndex,
	change_usage_refs: FxHashMap<Moniker, Vec<RefLocation>>,
}

pub(in crate::ui) struct ChangeIndexRefreshInput {
	opts: SessionOptions,
	index: Arc<SessionIndex>,
	search_docs: Arc<Vec<SearchDoc>>,
}

struct SearchDoc {
	loc: DefLocation,
	name: String,
	kind: String,
	path: String,
	moniker: String,
	signature: String,
}

impl MemoryIndexStore {
	pub(in crate::ui) fn load(opts: &SessionOptions) -> anyhow::Result<Self> {
		Ok(Self::new(SessionIndex::load(opts)?, opts.clone()))
	}

	pub(in crate::ui) fn catalog(opts: &SessionOptions) -> anyhow::Result<Self> {
		let sources = sources::discover(&opts.paths, opts.project.clone())?;
		Ok(Self::from_catalog_index(
			SessionIndex::catalog(sources),
			opts.clone(),
		))
	}

	pub(in crate::ui) fn empty(opts: SessionOptions) -> Self {
		Self::from_catalog_index(SessionIndex::empty(display_boot_path(&opts.paths)), opts)
	}

	fn new(index: SessionIndex, opts: SessionOptions) -> Self {
		let search_docs = build_search_docs(&index);
		let change_index = build_change_index(&index);
		let change_usage_refs = build_change_usage_refs(&index, &change_index);
		Self {
			opts,
			index: Arc::new(index),
			search_docs: Arc::new(search_docs),
			change_index,
			change_usage_refs,
		}
	}

	fn from_catalog_index(index: SessionIndex, opts: SessionOptions) -> Self {
		Self {
			opts,
			index: Arc::new(index),
			search_docs: Arc::new(Vec::new()),
			change_index: ChangeIndex::default(),
			change_usage_refs: FxHashMap::default(),
		}
	}

	pub(in crate::ui) fn options(&self) -> SessionOptions {
		self.opts.clone()
	}

	pub(in crate::ui) fn change_index_refresh_input(&self) -> ChangeIndexRefreshInput {
		ChangeIndexRefreshInput {
			opts: self.opts.clone(),
			index: Arc::clone(&self.index),
			search_docs: Arc::clone(&self.search_docs),
		}
	}

	pub(in crate::ui) fn refresh_change_indexed(input: ChangeIndexRefreshInput) -> Self {
		let change_index = build_change_index(&input.index);
		let change_usage_refs = build_change_usage_refs(&input.index, &change_index);
		Self {
			opts: input.opts,
			index: input.index,
			search_docs: input.search_docs,
			change_index,
			change_usage_refs,
		}
	}

	pub(in crate::ui) fn watch_roots(&self) -> Vec<StoreWatchRoot> {
		let ignored_paths = self
			.opts
			.cache_dir
			.as_ref()
			.map(|path| vec![absolute_path(path)])
			.unwrap_or_default();
		self.index
			.roots
			.iter()
			.enumerate()
			.map(|(idx, root)| StoreWatchRoot {
				path: root.path.clone(),
				git_root: self
					.change_index
					.resources
					.get(idx)
					.and_then(|resource| resource.git_root.clone()),
				ignored_paths: ignored_paths.clone(),
			})
			.collect()
	}

	pub(in crate::ui) fn refresh_change_index(&mut self) {
		self.change_index = build_change_index(&self.index);
		self.change_usage_refs = build_change_usage_refs(&self.index, &self.change_index);
	}

	pub(in crate::ui) fn reload(&mut self) -> anyhow::Result<()> {
		let index = SessionIndex::load(&self.opts)?;
		self.search_docs = Arc::new(build_search_docs(&index));
		self.change_index = build_change_index(&index);
		self.change_usage_refs = build_change_usage_refs(&index, &self.change_index);
		self.index = Arc::new(index);
		Ok(())
	}

	pub(in crate::ui) fn usage_focus_for_target(
		&self,
		target: Moniker,
		label: String,
	) -> UsageFocus {
		let refs = self.refs_matching_target(&target);
		let contexts = self.usage_contexts(&refs);
		UsageFocus {
			target,
			label,
			refs,
			contexts,
		}
	}
}

fn display_boot_path(paths: &[PathBuf]) -> String {
	match paths {
		[] => "<empty>".to_string(),
		[path] => path.display().to_string(),
		paths => paths
			.iter()
			.map(|path| path.display().to_string())
			.collect::<Vec<_>>()
			.join(", "),
	}
}

fn absolute_path(path: &Path) -> PathBuf {
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map(|cwd| cwd.join(path))
			.unwrap_or_else(|_| path.to_path_buf())
	}
}

fn build_change_index(index: &SessionIndex) -> ChangeIndex {
	let roots = index
		.roots
		.iter()
		.map(|root| ChangeRoot {
			label: &root.label,
			path: &root.path,
			ctx: &root.ctx,
		})
		.collect();
	let files = index
		.files
		.iter()
		.enumerate()
		.map(|(file_idx, file)| ChangeFile {
			file_idx,
			source_root: file.source_root,
			path: &file.path,
			rel_path: &file.rel_path,
			anchor: &file.anchor,
			lang: file.lang,
			graph: &file.graph,
			source: &file.source,
		})
		.collect();
	crate::ui::change::build_change_index(ChangeScan { roots, files })
}

fn build_change_usage_refs(
	index: &SessionIndex,
	change_index: &ChangeIndex,
) -> FxHashMap<Moniker, Vec<RefLocation>> {
	let mut cache = FxHashMap::default();
	for change in &change_index.entries {
		cache.entry(change.moniker.clone()).or_insert_with(|| {
			refs_matching_target_in_index(index, &change.moniker)
				.into_iter()
				.filter(|ref_loc| change_ref_is_outside_changed_symbol(index, change, ref_loc))
				.collect()
		});
	}
	cache
}

fn refs_matching_target_in_index(index: &SessionIndex, target: &Moniker) -> Vec<RefLocation> {
	let mut refs = Vec::new();
	for (file_idx, file) in index.files.iter().enumerate() {
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

fn change_ref_is_outside_changed_symbol(
	index: &SessionIndex,
	change: &ChangeEntry,
	ref_loc: &RefLocation,
) -> bool {
	if change.loc.is_none() {
		return true;
	}
	let reference = index.reference(ref_loc);
	let source = index.files[ref_loc.file].graph.def_at(reference.source);
	!change.moniker.bind_match(&source.moniker) && !change.moniker.is_ancestor_of(&source.moniker)
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
		self.sort_defs_for_navigation(&mut locs);
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
		self.sort_defs_for_navigation(&mut locs);
		locs
	}

	fn compare_defs_for_navigation(&self, left: &DefLocation, right: &DefLocation) -> Ordering {
		let left_def = self.def(left);
		let right_def = self.def(right);
		definition_kind_order(self.file(left.file).lang, &def_kind(left_def))
			.cmp(&definition_kind_order(
				self.file(right.file).lang,
				&def_kind(right_def),
			))
			.then_with(|| {
				left_def
					.position
					.map(|(start, _)| start)
					.cmp(&right_def.position.map(|(start, _)| start))
			})
			.then_with(|| last_name(&left_def.moniker).cmp(&last_name(&right_def.moniker)))
	}

	fn children_by_parent(&self, parent: &Moniker) -> &[DefLocation] {
		self.index
			.children_by_parent
			.get(parent)
			.map_or(&[], Vec::as_slice)
	}

	fn search_symbols(&self, query: &str, limit: usize) -> Vec<SearchHit> {
		let raw = query.trim().to_ascii_lowercase();
		let terms = search_terms(&raw);
		if raw.is_empty() || terms.is_empty() || limit == 0 {
			return Vec::new();
		}
		let mut hits: Vec<_> = self
			.search_docs
			.iter()
			.filter_map(|doc| {
				let (score, reason) = score_doc(doc, &raw, &terms)?;
				Some(SearchHit {
					loc: doc.loc,
					score,
					reason,
				})
			})
			.collect();
		hits.sort_by(|a, b| {
			b.score
				.cmp(&a.score)
				.then_with(|| self.def(&a.loc).moniker.cmp(&self.def(&b.loc).moniker))
		});
		hits.truncate(limit);
		hits
	}

	fn change_index(&self) -> &ChangeIndex {
		&self.change_index
	}

	fn changed_defs(&self) -> Vec<DefLocation> {
		self.change_index.changed_defs()
	}

	fn change_for_def(&self, loc: &DefLocation) -> Option<&ChangeEntry> {
		self.change_index.entry_for(loc)
	}

	fn change_count_for_file(&self, file_idx: usize) -> usize {
		self.change_index.change_count_for_file(file_idx)
	}

	fn change_usage_refs(&self, change: &ChangeEntry) -> &[RefLocation] {
		self.change_usage_refs
			.get(&change.moniker)
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
	fn sort_defs_for_navigation(&self, locs: &mut [DefLocation]) {
		locs.sort_by(|a, b| self.compare_defs_for_navigation(a, b));
	}

	fn refs_matching_target(&self, target: &Moniker) -> Vec<RefLocation> {
		refs_matching_target_in_index(&self.index, target)
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

fn build_search_docs(index: &SessionIndex) -> Vec<SearchDoc> {
	let mut docs = Vec::new();
	for (file_idx, file) in index.files.iter().enumerate() {
		for (def_idx, def) in file.graph.defs().enumerate() {
			if !is_navigable_def(file.lang, def) {
				continue;
			}
			let loc = DefLocation {
				file: file_idx,
				def: def_idx,
			};
			docs.push(SearchDoc {
				loc,
				name: last_name(&def.moniker).to_ascii_lowercase(),
				kind: def_kind(def).to_ascii_lowercase(),
				path: file.rel_path.display().to_string().to_ascii_lowercase(),
				moniker: compact_moniker(&def.moniker).to_ascii_lowercase(),
				signature: String::from_utf8_lossy(&def.signature).to_ascii_lowercase(),
			});
		}
	}
	docs
}

fn search_terms(query: &str) -> Vec<String> {
	query
		.split(|c: char| !c.is_alphanumeric())
		.filter(|term| !term.is_empty())
		.map(ToOwned::to_owned)
		.collect()
}

fn score_doc(doc: &SearchDoc, phrase: &str, terms: &[String]) -> Option<(u32, String)> {
	let fields = [
		("name", doc.name.as_str(), 120, 50),
		("kind", doc.kind.as_str(), 35, 20),
		("path", doc.path.as_str(), 25, 12),
		("moniker", doc.moniker.as_str(), 20, 10),
		("signature", doc.signature.as_str(), 10, 5),
	];
	let mut score = 0;
	let mut reason = None;
	for (label, value, exact_score, _) in fields {
		if value == phrase {
			score += exact_score * 2;
			reason.get_or_insert(label);
		} else if value.contains(phrase) {
			score += exact_score;
			reason.get_or_insert(label);
		}
	}
	for term in terms {
		let mut matched = false;
		for (label, value, _, term_score) in fields {
			if value.contains(term) {
				score += term_score;
				matched = true;
				reason.get_or_insert(label);
			}
		}
		if !matched {
			return None;
		}
	}
	(score > 0).then(|| (score, reason.unwrap_or("match").to_string()))
}

pub(in crate::ui) fn is_navigable_def(lang: Lang, def: &DefRecord) -> bool {
	is_navigable_definition(lang, &def_kind(def))
}

pub(in crate::ui) fn def_kind(def: &DefRecord) -> String {
	std::str::from_utf8(&def.kind).unwrap_or("?").to_string()
}

pub(in crate::ui) fn ref_kind(reference: &RefRecord) -> String {
	std::str::from_utf8(&reference.kind)
		.unwrap_or("?")
		.to_string()
}

pub(in crate::ui) fn last_name(moniker: &Moniker) -> String {
	moniker
		.as_view()
		.segments()
		.last()
		.and_then(|s| std::str::from_utf8(s.name).ok())
		.unwrap_or(".")
		.to_string()
}

pub(in crate::ui) fn compact_moniker(moniker: &Moniker) -> String {
	crate::format::render_compact_moniker(moniker, false).unwrap_or_else(|| {
		let cfg = code_moniker_core::core::uri::UriConfig {
			scheme: crate::DEFAULT_SCHEME,
		};
		crate::render_uri(moniker, &cfg)
	})
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
	let kind = std::str::from_utf8(selected_segment.kind).unwrap_or("");
	if !matches!(kind, "method" | "function" | "func" | "constructor") {
		return false;
	}
	bare_callable_name(selected_segment.name) == bare_callable_name(target_segment.name)
}

fn bare_callable_name(name: &[u8]) -> &[u8] {
	name.iter()
		.position(|b| *b == b'(')
		.map_or(name, |idx| &name[..idx])
}
