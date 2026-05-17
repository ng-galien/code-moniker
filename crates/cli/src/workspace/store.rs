use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::core::moniker::{Moniker, Segment};
use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use super::git::{ChangeEntry, ChangeFile, ChangeIndex, ChangeRoot, ChangeScan};
use super::index::{
	CheckSummary, DefLocation, IndexedFile, RefLocation, SessionIndex, SessionOptions, SessionStats,
};
use super::model::{
	ChangeBadge, ChangeDetail, ChangeId, ChangeOverview, ChangeSummary, FileSummary,
	GitResourceSummary, ReferenceDirection, ReferenceGroup, ReferenceSet, ReferenceSetSummary,
	SourceLine, SymbolDetail, SymbolReferences, SymbolSummary, UsageFocus,
};
use super::snapshot::{
	CoverageOverlay, GitOverlay, PlanOverlay, SearchDoc, SearchIndex, WorkspaceSnapshot,
};
use super::symbols::{compact_moniker, def_kind, is_navigable_def, last_name, ref_kind};
use crate::lines::line_range;
use crate::sources;

pub(crate) use super::model::SearchHit;

pub(crate) trait IndexStore {
	fn root(&self) -> &str;
	fn stats(&self) -> &SessionStats;
	fn file_count(&self) -> usize;
	fn file_summary(&self, file_idx: usize) -> FileSummary;
	fn all_navigable_defs(&self) -> Vec<DefLocation>;
	fn root_defs(&self, file_idx: usize) -> Vec<DefLocation>;
	fn child_defs(&self, parent: &DefLocation) -> Vec<DefLocation>;
	fn compare_defs_for_navigation(&self, left: &DefLocation, right: &DefLocation) -> Ordering;
	fn is_navigable_symbol(&self, loc: &DefLocation) -> bool;
	fn symbol_summary(&self, loc: &DefLocation) -> SymbolSummary;
	fn symbol_detail(&self, loc: &DefLocation) -> SymbolDetail;
	fn symbol_references(&self, loc: &DefLocation) -> SymbolReferences;
	fn source_snippet(&self, loc: &DefLocation, context: u32) -> Vec<SourceLine>;
	fn search_symbols_filtered(
		&self,
		query: &str,
		limit: usize,
		lang: Option<Lang>,
		kind: Option<&str>,
	) -> Vec<SearchHit>;
	fn change_overview(&self) -> ChangeOverview;
	fn change_rows(&self) -> Vec<ChangeSummary>;
	fn change_summary(&self, change: ChangeId) -> Option<ChangeSummary>;
	fn change_detail(&self, change: ChangeId) -> Option<ChangeDetail>;
	fn changed_defs(&self) -> Vec<DefLocation>;
	fn change_detail_for_symbol(&self, loc: &DefLocation) -> Option<ChangeDetail>;
	fn change_count_for_file(&self, file_idx: usize) -> usize;
	fn usage_focus(&self, loc: DefLocation) -> UsageFocus;
	fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoreWatchRoot {
	pub(crate) path: PathBuf,
	pub(crate) git_root: Option<PathBuf>,
	pub(crate) ignored_paths: Vec<PathBuf>,
}

#[derive(Clone)]
pub(crate) struct WorkspaceStore {
	opts: SessionOptions,
	snapshot: Arc<WorkspaceSnapshot>,
}

pub(crate) struct GitOverlayRefreshInput {
	index: Arc<SessionIndex>,
}

pub(crate) struct GitOverlayRefresh {
	index: Arc<SessionIndex>,
	git: GitOverlay,
}

impl WorkspaceStore {
	pub(crate) fn load(opts: &SessionOptions) -> anyhow::Result<Self> {
		Ok(Self::new(SessionIndex::load(opts)?, opts.clone()))
	}

	pub(crate) fn catalog(opts: &SessionOptions) -> anyhow::Result<Self> {
		let sources = sources::discover(&opts.paths, opts.project.clone())?;
		Ok(Self::from_catalog_index(
			SessionIndex::catalog(sources),
			opts.clone(),
		))
	}

	pub(crate) fn empty(opts: SessionOptions) -> Self {
		Self::from_catalog_index(SessionIndex::empty(display_boot_path(&opts.paths)), opts)
	}

	fn new(index: SessionIndex, opts: SessionOptions) -> Self {
		Self {
			opts,
			snapshot: Arc::new(build_snapshot(index)),
		}
	}

	fn from_catalog_index(index: SessionIndex, opts: SessionOptions) -> Self {
		Self {
			opts,
			snapshot: Arc::new(build_catalog_snapshot(index)),
		}
	}

	pub(crate) fn options(&self) -> SessionOptions {
		self.opts.clone()
	}

	pub(crate) fn git_overlay_refresh_input(&self) -> GitOverlayRefreshInput {
		GitOverlayRefreshInput {
			index: Arc::clone(&self.snapshot.index),
		}
	}

	pub(crate) fn build_git_overlay_refresh(input: GitOverlayRefreshInput) -> GitOverlayRefresh {
		let git = build_git_overlay(&input.index);
		GitOverlayRefresh {
			index: input.index,
			git,
		}
	}

	pub(crate) fn apply_git_overlay_refresh(&mut self, refresh: GitOverlayRefresh) -> bool {
		if !Arc::ptr_eq(&self.snapshot.index, &refresh.index) {
			return false;
		}
		self.snapshot = Arc::new(WorkspaceSnapshot {
			index: Arc::clone(&self.snapshot.index),
			search: Arc::clone(&self.snapshot.search),
			git: refresh.git,
			coverage: self.snapshot.coverage.clone(),
			plan: self.snapshot.plan.clone(),
		});
		true
	}

	pub(crate) fn watch_roots(&self) -> Vec<StoreWatchRoot> {
		let ignored_paths = self
			.opts
			.cache_dir
			.as_ref()
			.map(|path| vec![absolute_path(path)])
			.unwrap_or_default();
		self.snapshot
			.index
			.roots
			.iter()
			.enumerate()
			.map(|(idx, root)| StoreWatchRoot {
				path: root.path.clone(),
				git_root: self
					.snapshot
					.git
					.change_index
					.resources
					.get(idx)
					.and_then(|resource| resource.git_root.clone()),
				ignored_paths: ignored_paths.clone(),
			})
			.collect()
	}

	pub(crate) fn refresh_git_overlay(&mut self) {
		let git = build_git_overlay(&self.snapshot.index);
		self.snapshot = Arc::new(WorkspaceSnapshot {
			index: Arc::clone(&self.snapshot.index),
			search: Arc::clone(&self.snapshot.search),
			git,
			coverage: self.snapshot.coverage.clone(),
			plan: self.snapshot.plan.clone(),
		});
	}

	pub(crate) fn reload(&mut self) -> anyhow::Result<()> {
		let index = SessionIndex::load(&self.opts)?;
		self.snapshot = Arc::new(build_snapshot(index));
		Ok(())
	}

	pub(crate) fn usage_focus_for_target(&self, target: Moniker, label: String) -> UsageFocus {
		let refs = self.refs_matching_target(&target);
		let contexts = self.usage_contexts(&refs);
		let compact_moniker = compact_moniker(&target);
		let references = self.reference_set(&refs, ReferenceDirection::Incoming);
		UsageFocus {
			target,
			label,
			compact_moniker,
			refs,
			contexts,
			references,
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

fn build_snapshot(index: SessionIndex) -> WorkspaceSnapshot {
	let search = Arc::new(SearchIndex {
		docs: build_search_docs(&index),
	});
	let index = Arc::new(index);
	let git = build_git_overlay(&index);
	WorkspaceSnapshot {
		index,
		search,
		git,
		coverage: CoverageOverlay::default(),
		plan: PlanOverlay::default(),
	}
}

fn build_catalog_snapshot(index: SessionIndex) -> WorkspaceSnapshot {
	WorkspaceSnapshot {
		index: Arc::new(index),
		search: Arc::new(SearchIndex::default()),
		git: GitOverlay::default(),
		coverage: CoverageOverlay::default(),
		plan: PlanOverlay::default(),
	}
}

fn build_git_overlay(index: &SessionIndex) -> GitOverlay {
	let change_index = build_change_index(index);
	let change_usage_refs = build_change_usage_refs(index, &change_index);
	GitOverlay {
		change_index,
		change_usage_refs,
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
	super::git::build_change_index(ChangeScan { roots, files })
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
	for (candidate, locs) in &index.refs_by_target {
		if usage_target_matches(target, candidate) {
			refs.extend(locs.iter().copied());
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

impl IndexStore for WorkspaceStore {
	fn root(&self) -> &str {
		&self.snapshot.index.root
	}

	fn stats(&self) -> &SessionStats {
		&self.snapshot.index.stats
	}

	fn file_count(&self) -> usize {
		self.snapshot.index.files.len()
	}

	fn file_summary(&self, file_idx: usize) -> FileSummary {
		let file = self.raw_file(file_idx);
		FileSummary {
			index: file_idx,
			lang: file.lang,
			rel_path: file.rel_path.clone(),
			anchor: file.anchor.clone(),
		}
	}

	fn all_navigable_defs(&self) -> Vec<DefLocation> {
		let mut out: Vec<DefLocation> = self
			.snapshot
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
				let def = self.raw_def(loc);
				is_navigable_def(self.raw_file(loc.file).lang, def)
			})
			.collect();
		out.sort_by(|a, b| self.raw_def(a).moniker.cmp(&self.raw_def(b).moniker));
		out
	}

	fn root_defs(&self, file_idx: usize) -> Vec<DefLocation> {
		let mut locs: Vec<DefLocation> = self.snapshot.index.files[file_idx]
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
			.snapshot
			.index
			.children_by_parent
			.get(&self.raw_def(parent).moniker)
			.into_iter()
			.flat_map(|children| children.iter().copied())
			.filter(|loc| loc.file == parent.file)
			.collect();
		self.sort_defs_for_navigation(&mut locs);
		locs
	}

	fn compare_defs_for_navigation(&self, left: &DefLocation, right: &DefLocation) -> Ordering {
		let left_def = self.raw_def(left);
		let right_def = self.raw_def(right);
		definition_kind_order(self.raw_file(left.file).lang, &def_kind(left_def))
			.cmp(&definition_kind_order(
				self.raw_file(right.file).lang,
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

	fn is_navigable_symbol(&self, loc: &DefLocation) -> bool {
		is_navigable_def(self.raw_file(loc.file).lang, self.raw_def(loc))
	}

	fn symbol_summary(&self, loc: &DefLocation) -> SymbolSummary {
		self.symbol_summary_for_loc(loc)
	}

	fn symbol_detail(&self, loc: &DefLocation) -> SymbolDetail {
		SymbolDetail {
			symbol: self.symbol_summary_for_loc(loc),
			children: self
				.children_by_parent_raw(&self.raw_def(loc).moniker)
				.iter()
				.copied()
				.filter(|child| child.file == loc.file)
				.map(|child| self.symbol_summary_for_loc(&child))
				.collect(),
		}
	}

	fn symbol_references(&self, loc: &DefLocation) -> SymbolReferences {
		let symbol = self.symbol_summary_for_loc(loc);
		let moniker = &self.raw_def(loc).moniker;
		SymbolReferences {
			symbol,
			incoming: self.reference_set(
				self.snapshot.index.incoming_refs(moniker),
				ReferenceDirection::Incoming,
			),
			outgoing: self.reference_set(
				self.snapshot.index.outgoing_refs(moniker),
				ReferenceDirection::Outgoing,
			),
		}
	}

	fn source_snippet(&self, loc: &DefLocation, context: u32) -> Vec<SourceLine> {
		let file = self.raw_file(loc.file);
		let Some((start, end)) = self.raw_def(loc).position else {
			return Vec::new();
		};
		let (start_line, end_line) = line_range(&file.source, start, end);
		let first = start_line.saturating_sub(context).max(1);
		let last = end_line.saturating_add(context);
		file.source
			.lines()
			.enumerate()
			.filter_map(|(idx, text)| {
				let number = idx as u32 + 1;
				(first <= number && number <= last).then(|| SourceLine {
					number,
					text: text.to_string(),
					active: start_line <= number && number <= end_line,
				})
			})
			.collect()
	}

	fn search_symbols_filtered(
		&self,
		query: &str,
		limit: usize,
		lang: Option<Lang>,
		kind: Option<&str>,
	) -> Vec<SearchHit> {
		self.search_symbols_matching(query, limit, |doc| {
			let file_lang = self.raw_file(doc.loc.file).lang;
			let def = self.raw_def(&doc.loc);
			lang.is_none_or(|lang| file_lang == lang)
				&& kind.is_none_or(|kind| def_kind(def) == kind)
		})
	}

	fn change_overview(&self) -> ChangeOverview {
		let changes = &self.snapshot.git.change_index;
		ChangeOverview {
			scope: changes.scope.clone(),
			change_count: changes.entries.len(),
			file_count: changes.changed_file_count(),
			resources: changes
				.resources
				.iter()
				.map(|resource| GitResourceSummary {
					available: resource.available(),
					label: resource.label.clone(),
					message: resource.message.clone(),
				})
				.collect(),
			diagnostics: changes.diagnostics.clone(),
		}
	}

	fn change_rows(&self) -> Vec<ChangeSummary> {
		self.snapshot
			.git
			.change_index
			.entries
			.iter()
			.enumerate()
			.map(|(idx, change)| self.change_summary_for_entry(ChangeId::new(idx), change))
			.collect()
	}

	fn change_summary(&self, change: ChangeId) -> Option<ChangeSummary> {
		self.snapshot
			.git
			.change_index
			.entries
			.get(change.index())
			.map(|entry| self.change_summary_for_entry(change, entry))
	}

	fn change_detail(&self, change: ChangeId) -> Option<ChangeDetail> {
		self.snapshot
			.git
			.change_index
			.entries
			.get(change.index())
			.map(|entry| self.change_detail_for_entry(change, entry))
	}

	fn changed_defs(&self) -> Vec<DefLocation> {
		self.snapshot.git.change_index.changed_defs()
	}

	fn change_detail_for_symbol(&self, loc: &DefLocation) -> Option<ChangeDetail> {
		self.snapshot
			.git
			.change_index
			.entry_for(loc)
			.and_then(|entry| self.change_id_for_entry(entry).map(|id| (id, entry)))
			.map(|(id, entry)| self.change_detail_for_entry(id, entry))
	}

	fn change_count_for_file(&self, file_idx: usize) -> usize {
		self.snapshot
			.git
			.change_index
			.change_count_for_file(file_idx)
	}

	fn usage_focus(&self, loc: DefLocation) -> UsageFocus {
		let target = self.raw_def(&loc).moniker.clone();
		let label = last_name(&target);
		let compact_moniker = compact_moniker(&target);
		let refs = self.refs_matching_target(&target);
		let contexts = self.usage_contexts(&refs);
		let references = self.reference_set(&refs, ReferenceDirection::Incoming);
		UsageFocus {
			target,
			label,
			compact_moniker,
			refs,
			contexts,
			references,
		}
	}

	fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary> {
		self.snapshot.index.check_summary(rules, profile, scheme)
	}
}

impl WorkspaceStore {
	fn search_symbols_matching(
		&self,
		query: &str,
		limit: usize,
		mut include: impl FnMut(&SearchDoc) -> bool,
	) -> Vec<SearchHit> {
		let raw = query.trim().to_ascii_lowercase();
		let terms = search_terms(&raw);
		if raw.is_empty() || terms.is_empty() || limit == 0 {
			return Vec::new();
		}
		let mut hits: Vec<_> = self
			.snapshot
			.search
			.docs
			.iter()
			.filter(|doc| include(doc))
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
			b.score.cmp(&a.score).then_with(|| {
				self.raw_def(&a.loc)
					.moniker
					.cmp(&self.raw_def(&b.loc).moniker)
			})
		});
		hits.truncate(limit);
		hits
	}
	fn sort_defs_for_navigation(&self, locs: &mut [DefLocation]) {
		locs.sort_by(|a, b| self.compare_defs_for_navigation(a, b));
	}

	fn raw_file(&self, file_idx: usize) -> &IndexedFile {
		&self.snapshot.index.files[file_idx]
	}

	fn raw_def(&self, loc: &DefLocation) -> &DefRecord {
		self.snapshot.index.def(loc)
	}

	fn raw_reference(&self, loc: &RefLocation) -> &RefRecord {
		self.snapshot.index.reference(loc)
	}

	fn children_by_parent_raw(&self, parent: &Moniker) -> &[DefLocation] {
		self.snapshot
			.index
			.children_by_parent
			.get(parent)
			.map_or(&[], Vec::as_slice)
	}

	fn symbol_summary_for_loc(&self, loc: &DefLocation) -> SymbolSummary {
		let file = self.raw_file(loc.file);
		let def = self.raw_def(loc);
		SymbolSummary {
			id: *loc,
			lang: file.lang,
			kind: def_kind(def),
			name: last_name(&def.moniker),
			file_path: file.rel_path.clone(),
			compact_moniker: compact_moniker(&def.moniker),
			line_range: def
				.position
				.map(|(start, end)| line_range(&file.source, start, end)),
			child_count: self
				.children_by_parent_raw(&def.moniker)
				.iter()
				.filter(|child| child.file == loc.file)
				.count(),
			change: self.change_badge_for_loc(loc),
		}
	}

	fn change_badge_for_loc(&self, loc: &DefLocation) -> Option<ChangeBadge> {
		let entry = self.snapshot.git.change_index.entry_for(loc)?;
		Some(ChangeBadge {
			status: entry.status,
			usage_count: self.change_usage_refs_for_entry(entry).len(),
		})
	}

	fn change_id_for_entry(&self, entry: &ChangeEntry) -> Option<ChangeId> {
		self.snapshot
			.git
			.change_index
			.entries
			.iter()
			.position(|candidate| std::ptr::eq(candidate, entry))
			.map(ChangeId::new)
	}

	fn change_summary_for_entry(&self, id: ChangeId, entry: &ChangeEntry) -> ChangeSummary {
		ChangeSummary {
			id,
			status: entry.status,
			lang: entry.lang,
			kind: entry.kind.clone(),
			name: entry.name.clone(),
			file_path: entry.file_path.clone(),
			compact_moniker: compact_moniker(&entry.moniker),
			line_range: entry.line_range,
			hunk_count: entry.hunk_count,
			usage_count: self.change_usage_refs_for_entry(entry).len(),
		}
	}

	fn change_detail_for_entry(&self, id: ChangeId, entry: &ChangeEntry) -> ChangeDetail {
		ChangeDetail {
			summary: self.change_summary_for_entry(id, entry),
			blast_radius: self.reference_set(
				self.change_usage_refs_for_entry(entry),
				ReferenceDirection::Incoming,
			),
		}
	}

	fn change_usage_refs_for_entry(&self, change: &ChangeEntry) -> &[RefLocation] {
		self.snapshot
			.git
			.change_usage_refs
			.get(&change.moniker)
			.map_or(&[], Vec::as_slice)
	}

	fn reference_set(&self, refs: &[RefLocation], direction: ReferenceDirection) -> ReferenceSet {
		ReferenceSet {
			summary: self.reference_set_summary(refs),
			groups: self.reference_groups(refs, direction),
		}
	}

	fn reference_set_summary(&self, refs: &[RefLocation]) -> ReferenceSetSummary {
		let files = refs
			.iter()
			.map(|loc| loc.file)
			.collect::<std::collections::BTreeSet<_>>()
			.len();
		ReferenceSetSummary {
			refs: refs.len(),
			files,
			contexts: self.usage_contexts(refs).len(),
		}
	}

	fn reference_groups(
		&self,
		refs: &[RefLocation],
		direction: ReferenceDirection,
	) -> Vec<ReferenceGroup> {
		let mut groups: Vec<ReferenceGroup> = Vec::new();
		for loc in refs {
			let group = self.reference_group(loc, direction);
			if let Some(existing) = groups
				.iter_mut()
				.find(|existing| reference_groups_same_context(existing, &group))
			{
				for kind in group.kinds {
					if !existing.kinds.contains(&kind) {
						existing.kinds.push(kind);
					}
				}
			} else {
				groups.push(group);
			}
		}
		for group in &mut groups {
			sort_reference_kinds(&mut group.kinds);
		}
		groups
	}

	fn reference_group(&self, loc: &RefLocation, direction: ReferenceDirection) -> ReferenceGroup {
		let file = self.raw_file(loc.file);
		let reference = self.raw_reference(loc);
		let source = file.graph.def_at(reference.source);
		let kind = ref_kind(reference);
		let actor = match direction {
			ReferenceDirection::Incoming => last_name(&source.moniker),
			ReferenceDirection::Outgoing => last_name(&reference.target),
		};
		let endpoint_label = match direction {
			ReferenceDirection::Incoming => "source",
			ReferenceDirection::Outgoing => "target",
		};
		let endpoint = match direction {
			ReferenceDirection::Incoming => compact_moniker(&source.moniker),
			ReferenceDirection::Outgoing => compact_moniker(&reference.target),
		};
		ReferenceGroup {
			kinds: vec![kind],
			actor,
			location: self.reference_location(loc),
			endpoint_label,
			endpoint,
			confidence: ref_confidence(reference),
			receiver: ref_attr(&reference.receiver_hint).map(str::to_string),
			alias: ref_attr(&reference.alias).map(str::to_string),
		}
	}

	fn reference_location(&self, loc: &RefLocation) -> String {
		let file = self.raw_file(loc.file);
		let reference = self.raw_reference(loc);
		let lines = reference
			.position
			.map(|(start, end)| {
				let (start_line, end_line) = line_range(&file.source, start, end);
				if start_line == end_line {
					format!("L{start_line}")
				} else {
					format!("L{start_line}-L{end_line}")
				}
			})
			.unwrap_or_else(|| "L?".to_string());
		format!("{}:{lines}", file.rel_path.display())
	}

	fn refs_matching_target(&self, target: &Moniker) -> Vec<RefLocation> {
		refs_matching_target_in_index(&self.snapshot.index, target)
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
			self.raw_file(a.file)
				.rel_path
				.cmp(&self.raw_file(b.file).rel_path)
				.then_with(|| self.raw_def(a).moniker.cmp(&self.raw_def(b).moniker))
		});
		out
	}

	fn nav_contexts_for_ref(&self, loc: &RefLocation) -> Vec<DefLocation> {
		let reference = self.raw_reference(loc);
		let source = DefLocation {
			file: loc.file,
			def: reference.source,
		};
		if is_navigable_def(self.raw_file(source.file).lang, self.raw_def(&source)) {
			return vec![source];
		}
		let source_moniker = self.raw_def(&source).moniker.clone();
		self.children_by_parent_raw(&source_moniker)
			.iter()
			.copied()
			.filter(|child| {
				child.file == loc.file
					&& is_navigable_def(self.raw_file(child.file).lang, self.raw_def(child))
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

fn reference_groups_same_context(left: &ReferenceGroup, right: &ReferenceGroup) -> bool {
	left.actor == right.actor
		&& left.location == right.location
		&& left.endpoint_label == right.endpoint_label
		&& left.endpoint == right.endpoint
		&& left.confidence == right.confidence
		&& left.receiver == right.receiver
		&& left.alias == right.alias
}

fn ref_confidence(reference: &RefRecord) -> String {
	ref_attr(&reference.confidence)
		.map(str::to_string)
		.unwrap_or_else(|| "-".to_string())
}

fn ref_attr(bytes: &[u8]) -> Option<&str> {
	if bytes.is_empty() {
		return None;
	}
	std::str::from_utf8(bytes).ok().filter(|s| !s.is_empty())
}

fn sort_reference_kinds(kinds: &mut [String]) {
	kinds.sort_by(|left, right| {
		reference_kind_order(left)
			.cmp(&reference_kind_order(right))
			.then_with(|| left.cmp(right))
	});
}

fn reference_kind_order(kind: &str) -> u16 {
	match kind {
		"extends" | "implements" => 10,
		"instantiates" => 20,
		"uses_type" | "annotates" => 30,
		"calls" | "method_call" => 40,
		"reads" => 50,
		"imports_symbol" | "imports_module" | "reexports" => 60,
		"di_register" | "di_require" => 70,
		_ => 90,
	}
}

fn definition_kind_order(lang: Lang, kind: &str) -> u16 {
	lang.kind_spec(kind)
		.map(|spec| spec.order)
		.or_else(|| shape_of(kind.as_bytes()).map(fallback_order_for_shape))
		.unwrap_or(u16::MAX)
}

fn fallback_order_for_shape(shape: Shape) -> u16 {
	match shape {
		Shape::Namespace => 10,
		Shape::Type => 20,
		Shape::Callable => 40,
		Shape::Value => 60,
		Shape::Ref => 80,
		Shape::Annotation => 90,
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

#[cfg(test)]
mod tests {
	use super::*;

	fn write(root: &Path, rel: &str, body: &str) {
		let p = root.join(rel);
		if let Some(parent) = p.parent() {
			std::fs::create_dir_all(parent).unwrap();
		}
		std::fs::write(p, body).unwrap();
	}

	#[test]
	fn workspace_store_builds_snapshot_with_query_and_overlays() {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"src/user.ts",
			"export class UserService { find() { return 1; } }\n",
		);

		let store = WorkspaceStore::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();

		assert_eq!(store.stats().files, 1);
		assert!(!store.snapshot.search.docs.is_empty());
		assert_eq!(store.snapshot.git.change_index.scope, "HEAD..worktree");
		assert_eq!(store.snapshot.coverage.generation, 0);
		assert_eq!(store.snapshot.plan.generation, 0);
		assert!(
			store
				.search_symbols_filtered("UserService", 5, None, None)
				.iter()
				.any(|hit| store.symbol_summary(&hit.loc).name == "UserService")
		);
	}

	#[test]
	fn catalog_snapshot_keeps_files_without_symbol_search_docs() {
		let tmp = tempfile::tempdir().unwrap();
		write(tmp.path(), "src/user.ts", "export class UserService {}\n");

		let store = WorkspaceStore::catalog(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();

		assert_eq!(store.stats().files, 1);
		assert!(store.snapshot.search.docs.is_empty());
		assert_eq!(store.change_overview().change_count, 0);
	}

	#[test]
	fn change_refresh_applies_as_git_overlay_patch_only() {
		let tmp = tempfile::tempdir().unwrap();
		write(tmp.path(), "src/user.ts", "export class UserService {}\n");
		let mut store = WorkspaceStore::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();

		let search = Arc::clone(&store.snapshot.search);
		let input = store.git_overlay_refresh_input();
		let refresh = WorkspaceStore::build_git_overlay_refresh(input);

		assert!(store.apply_git_overlay_refresh(refresh));
		assert!(Arc::ptr_eq(&search, &store.snapshot.search));
		assert_eq!(store.snapshot.coverage.generation, 0);
		assert_eq!(store.snapshot.plan.generation, 0);
	}

	#[test]
	fn stale_change_refresh_does_not_patch_a_reloaded_index() {
		let tmp = tempfile::tempdir().unwrap();
		write(tmp.path(), "src/user.ts", "export class UserService {}\n");
		let mut store = WorkspaceStore::load(&SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		})
		.unwrap();

		let input = store.git_overlay_refresh_input();
		store.reload().unwrap();
		let refresh = WorkspaceStore::build_git_overlay_refresh(input);

		assert!(!store.apply_git_overlay_refresh(refresh));
	}
}
