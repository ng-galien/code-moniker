// code-moniker: ignore-file[smell-feature-envy-local, smell-god-type-local-metrics, smell-large-type]
// Compatibility bridge for the legacy `IndexStore` surface. The target session
// model stays independent; this anti-corruption layer is the only module that
// translates the new contract back into legacy UI read-model types.
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::core::uri::{UriConfig, from_uri};
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::check::workspace::{WorkspaceCheckRunner, WorkspaceCheckRunnerOptions};
use crate::workspace::changes::LocalChangeOverlay;
use crate::workspace::code::compact_moniker;
use crate::workspace::code::{LocalCodeIndex, LocalCodeIndexOptions};
use crate::workspace::git::ChangeStatus as LegacyChangeStatus;
use crate::workspace::index::{
	CheckSummary, DefLocation, RefLocation, SessionOptions, SessionStats,
};
use crate::workspace::legacy::linkage::LinkageStats;
use crate::workspace::linkage::LocalLinkage;
use crate::workspace::model::{
	ChangeBadge, ChangeDetail, ChangeId as LegacyChangeId, ChangeOverview, ChangeSummary,
	FileSummary, GitResourceSummary, ReferenceDirection, ReferenceGroup, ReferenceSet,
	ReferenceSetSummary, SearchHit, SourceLine, SymbolDetail, SymbolReferences, SymbolSummary,
	UnresolvedLinkageGroup, UnresolvedLinkageReport, UnresolvedLinkageSample, UsageFocus,
};
use crate::workspace::snapshot::{
	ChangeOverlay, ChangeRecord, ChangeStatus, CodeIndex, LinkageGraph, ReferenceId,
	ReferenceRecord, ResourceGeneration, SourceCatalog, SourceFileRecord, SourceId, SymbolId,
	SymbolRecord, WorkspaceRequest, WorkspaceSnapshot, WorkspaceSnapshotRefresh,
	WorkspaceTransition,
};
use crate::workspace::source::{LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions};
use crate::workspace::store::IndexStore;

#[derive(Clone)]
pub struct SessionStoreBridge {
	opts: SessionOptions,
	root: String,
	snapshot: WorkspaceSnapshot,
	cache: Option<LocalResourceCache>,
	stats: SessionStats,
	linkage_stats: LinkageStats,
	symbols_by_loc: FxHashMap<DefLocation, SymbolId>,
	refs_by_loc: FxHashMap<RefLocation, ReferenceId>,
}

impl SessionStoreBridge {
	pub fn load(opts: SessionOptions) -> anyhow::Result<Self> {
		let cache = LocalResourceCache::default();
		let mut session = WorkspaceSnapshotRefresh::new(
			LocalSourceCatalog::new(
				LocalSourceCatalogOptions::new(opts.paths.clone(), opts.project.clone()),
				cache.clone(),
			),
			LocalCodeIndex::new(
				LocalCodeIndexOptions::new(opts.cache_dir.clone()),
				cache.clone(),
			),
			LocalLinkage::new(cache.clone()),
			LocalChangeOverlay::new(cache.clone()),
		);
		match session.refresh(WorkspaceRequest::new("workspace")) {
			WorkspaceTransition::Ready { .. } => {
				let snapshot = session
					.snapshot()
					.expect("ready transition has snapshot")
					.clone();
				Ok(Self::from_snapshot_with_cache(opts, snapshot, Some(cache)))
			}
			WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
		}
	}

	pub fn from_snapshot(opts: SessionOptions, snapshot: WorkspaceSnapshot) -> Self {
		Self::from_snapshot_with_cache(opts, snapshot, None)
	}

	pub fn empty(opts: SessionOptions) -> Self {
		let generation = ResourceGeneration::new(0);
		let catalog = SourceCatalog::new(generation, Vec::new());
		let index = CodeIndex::from_fields(crate::workspace::snapshot::CodeIndexFields {
			generation,
			catalog_generation: generation,
			identity_scheme: crate::DEFAULT_SCHEME.to_string(),
			sources: Vec::new(),
			symbols: Vec::new(),
			references: Vec::new(),
		});
		let linkage = LinkageGraph::new(generation, generation, 0, 0);
		let changes = ChangeOverlay::new(generation, generation, generation, Vec::new());
		Self::from_snapshot(
			opts,
			WorkspaceSnapshot {
				generation,
				catalog,
				index,
				linkage,
				changes,
			},
		)
	}

	fn from_snapshot_with_cache(
		opts: SessionOptions,
		snapshot: WorkspaceSnapshot,
		cache: Option<LocalResourceCache>,
	) -> Self {
		let root = display_boot_path(&opts.paths);
		let stats = build_stats(&snapshot);
		let linkage_stats = LinkageStats {
			resolved_refs: snapshot.linkage.resolved_refs,
			external_refs: snapshot.linkage.external_refs,
			manifest_blocked_refs: snapshot.linkage.manifest_blocked_refs,
			unresolved_refs: snapshot.linkage.unresolved_refs,
			ambiguous_refs: snapshot.linkage.ambiguous_refs,
		};
		let symbols_by_loc = snapshot
			.index
			.symbols
			.iter()
			.filter_map(|symbol| loc_for_symbol_id(&symbol.id).map(|loc| (loc, symbol.id.clone())))
			.collect();
		let refs_by_loc = snapshot
			.index
			.references
			.iter()
			.filter_map(|reference| {
				loc_for_reference_id(&reference.id).map(|loc| (loc, reference.id.clone()))
			})
			.collect();
		Self {
			opts,
			root,
			snapshot,
			cache,
			stats,
			linkage_stats,
			symbols_by_loc,
			refs_by_loc,
		}
	}

	pub fn options(&self) -> SessionOptions {
		self.opts.clone()
	}

	pub(crate) fn usage_focus_for_target(&self, target: Moniker, label: String) -> UsageFocus {
		let compact_moniker = compact_moniker(&target);
		let refs = self
			.snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| self.moniker_for_identity(&symbol.identity).as_ref() == Some(&target))
			.map(|symbol| self.incoming_refs_for_symbol(&symbol.id))
			.unwrap_or_default();
		let contexts = self.usage_contexts(&refs);
		let references = self.reference_set(&refs, ReferenceDirection::Incoming);
		UsageFocus {
			target,
			label,
			compact_moniker,
			contexts,
			references,
			refs,
		}
	}

	fn source_file(&self, file_idx: usize) -> &SourceFileRecord {
		&self.snapshot.index.sources[file_idx]
	}

	fn symbol_for_loc(&self, loc: &DefLocation) -> &SymbolRecord {
		let id = self
			.symbols_by_loc
			.get(loc)
			.expect("def location belongs to session snapshot");
		self.snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| &symbol.id == id)
			.expect("symbol id belongs to session snapshot")
	}

	fn reference_for_loc(&self, loc: &RefLocation) -> &ReferenceRecord {
		let id = self
			.refs_by_loc
			.get(loc)
			.expect("ref location belongs to session snapshot");
		self.snapshot
			.index
			.references
			.iter()
			.find(|reference| &reference.id == id)
			.expect("reference id belongs to session snapshot")
	}

	fn symbol_loc(&self, id: &SymbolId) -> Option<DefLocation> {
		loc_for_symbol_id(id)
	}

	fn reference_loc(&self, id: &ReferenceId) -> Option<RefLocation> {
		loc_for_reference_id(id)
	}

	fn symbol_id_for_loc(&self, loc: &DefLocation) -> Option<&SymbolId> {
		self.symbols_by_loc.get(loc)
	}

	fn source_lang(&self, source: &SourceFileRecord) -> Lang {
		Lang::from_tag(&source.language).unwrap_or(Lang::Rs)
	}

	fn symbol_lang(&self, symbol: &SymbolRecord) -> Lang {
		self.snapshot
			.index
			.sources
			.iter()
			.find(|source| source.id == symbol.source)
			.map(|source| self.source_lang(source))
			.unwrap_or(Lang::Rs)
	}

	fn symbol_source_path(&self, symbol: &SymbolRecord) -> PathBuf {
		self.snapshot
			.index
			.sources
			.iter()
			.find(|source| source.id == symbol.source)
			.map(|source| PathBuf::from(&source.rel_path))
			.unwrap_or_default()
	}

	fn symbol_summary_for_record(&self, symbol: &SymbolRecord) -> SymbolSummary {
		let loc = self
			.symbol_loc(&symbol.id)
			.expect("symbol id uses bridge location format");
		SymbolSummary {
			id: loc,
			lang: self.symbol_lang(symbol),
			kind: symbol.kind.clone(),
			name: symbol.name.clone(),
			file_path: self.symbol_source_path(symbol),
			compact_moniker: self.compact_identity(&symbol.identity),
			line_range: symbol.line_range,
			child_count: self.children_for_symbol(&symbol.id).len(),
			change: self.change_badge_for_symbol(&symbol.id),
		}
	}

	fn children_for_symbol(&self, parent: &SymbolId) -> Vec<DefLocation> {
		let Some(parent_loc) = self.symbol_loc(parent) else {
			return Vec::new();
		};
		let mut children = self
			.snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.parent.as_ref() == Some(parent))
			.filter_map(|symbol| self.symbol_loc(&symbol.id))
			.filter(|loc| loc.file == parent_loc.file)
			.collect::<Vec<_>>();
		self.sort_defs_for_navigation(&mut children);
		children
	}

	fn incoming_refs_for_symbol(&self, symbol: &SymbolId) -> Vec<RefLocation> {
		let mut refs = self
			.snapshot
			.linkage
			.resolved
			.iter()
			.filter(|edge| &edge.target == symbol)
			.filter_map(|edge| self.reference_loc(&edge.reference))
			.collect::<Vec<_>>();
		refs.sort_by_key(|loc| (self.source_file(loc.file).rel_path.clone(), loc.reference));
		refs
	}

	fn outgoing_refs_for_symbol(&self, symbol: &SymbolId) -> Vec<RefLocation> {
		let mut refs = self
			.snapshot
			.index
			.references
			.iter()
			.filter(|reference| &reference.source_symbol == symbol)
			.filter_map(|reference| self.reference_loc(&reference.id))
			.collect::<Vec<_>>();
		refs.sort_by_key(|loc| (self.source_file(loc.file).rel_path.clone(), loc.reference));
		refs
	}

	fn source_line_range(&self, loc: &RefLocation) -> Option<(u32, u32)> {
		self.reference_for_loc(loc).line_range
	}

	fn reference_location(&self, loc: &RefLocation) -> String {
		let source = self.source_file(loc.file);
		let lines = self
			.source_line_range(loc)
			.map(|(start, end)| {
				if start == end {
					format!("L{start}")
				} else {
					format!("L{start}-L{end}")
				}
			})
			.unwrap_or_else(|| "L?".to_string());
		format!("{}:{lines}", source.rel_path)
	}

	fn reference_set(&self, refs: &[RefLocation], direction: ReferenceDirection) -> ReferenceSet {
		ReferenceSet {
			summary: self.reference_set_summary(refs),
			groups: self.reference_groups(refs, direction),
		}
	}

	fn reference_set_summary(&self, refs: &[RefLocation]) -> ReferenceSetSummary {
		ReferenceSetSummary {
			refs: refs.len(),
			files: refs
				.iter()
				.map(|loc| loc.file)
				.collect::<BTreeSet<_>>()
				.len(),
			contexts: self.usage_contexts(refs).len(),
		}
	}

	fn reference_groups(
		&self,
		refs: &[RefLocation],
		direction: ReferenceDirection,
	) -> Vec<ReferenceGroup> {
		let mut groups = Vec::<ReferenceGroup>::new();
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
		let reference = self.reference_for_loc(loc);
		let source = self.symbol_by_id(&reference.source_symbol);
		let kind = reference.kind.clone();
		let actor = match direction {
			ReferenceDirection::Incoming => source
				.map(|symbol| symbol.name.clone())
				.unwrap_or_else(|| reference.source_symbol.as_str().to_string()),
			ReferenceDirection::Outgoing => self.compact_identity(&reference.target_identity),
		};
		let endpoint_label = match direction {
			ReferenceDirection::Incoming => "source",
			ReferenceDirection::Outgoing => "target",
		};
		let endpoint = match direction {
			ReferenceDirection::Incoming => source
				.map(|symbol| self.compact_identity(&symbol.identity))
				.unwrap_or_else(|| reference.source_symbol.as_str().to_string()),
			ReferenceDirection::Outgoing => self.compact_identity(&reference.target_identity),
		};
		ReferenceGroup {
			kinds: vec![kind],
			actor,
			location: self.reference_location(loc),
			endpoint_label,
			endpoint,
			confidence: reference
				.confidence
				.clone()
				.unwrap_or_else(|| "-".to_string()),
			receiver: reference.receiver.clone(),
			alias: reference.alias.clone(),
		}
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
		out.sort_by(|left, right| {
			self.source_file(left.file)
				.rel_path
				.cmp(&self.source_file(right.file).rel_path)
				.then_with(|| {
					self.symbol_for_loc(left)
						.identity
						.cmp(&self.symbol_for_loc(right).identity)
				})
		});
		out
	}

	fn nav_contexts_for_ref(&self, loc: &RefLocation) -> Vec<DefLocation> {
		let reference = self.reference_for_loc(loc);
		let Some(source_loc) = self.symbol_loc(&reference.source_symbol) else {
			return Vec::new();
		};
		let source = self.symbol_for_loc(&source_loc);
		if source.navigable {
			return vec![source_loc];
		}
		self.children_for_symbol(&source.id)
	}

	fn change_badge_for_symbol(&self, symbol: &SymbolId) -> Option<ChangeBadge> {
		let change = self
			.snapshot
			.changes
			.changes
			.iter()
			.find(|change| change.symbol.as_ref() == Some(symbol))?;
		Some(ChangeBadge {
			status: legacy_change_status(change.status),
			usage_count: self.change_usage_refs(change).len(),
		})
	}

	fn change_usage_refs(&self, change: &ChangeRecord) -> Vec<RefLocation> {
		let Some(symbol) = change.symbol.as_ref() else {
			return Vec::new();
		};
		self.incoming_refs_for_symbol(symbol)
			.into_iter()
			.filter(|loc| {
				let reference = self.reference_for_loc(loc);
				reference.source_symbol != *symbol
			})
			.collect()
	}

	fn change_summary_for_record(&self, idx: usize, change: &ChangeRecord) -> ChangeSummary {
		let source = change
			.source
			.as_ref()
			.and_then(|source| self.source_file_by_id(source));
		ChangeSummary {
			id: LegacyChangeId::new(idx),
			status: legacy_change_status(change.status),
			lang: source
				.map(|source| self.source_lang(source))
				.or_else(|| Lang::from_tag(&change.language))
				.unwrap_or(Lang::Rs),
			kind: change.kind.clone(),
			name: change.name.clone(),
			file_path: source
				.map(|source| PathBuf::from(&source.rel_path))
				.unwrap_or_else(|| PathBuf::from(&change.file_path)),
			compact_moniker: self.compact_identity(&change.identity),
			line_range: change.line_range,
			hunk_count: change.hunk_count,
			usage_count: self.change_usage_refs(change).len(),
		}
	}

	fn change_detail_for_record(&self, idx: usize, change: &ChangeRecord) -> ChangeDetail {
		ChangeDetail {
			summary: self.change_summary_for_record(idx, change),
			blast_radius: self.reference_set(
				&self.change_usage_refs(change),
				ReferenceDirection::Incoming,
			),
		}
	}

	fn source_file_by_id(&self, source: &SourceId) -> Option<&SourceFileRecord> {
		self.snapshot
			.index
			.sources
			.iter()
			.find(|candidate| &candidate.id == source)
	}

	fn source_file_index_by_id(&self, source: &SourceId) -> Option<usize> {
		self.snapshot
			.index
			.sources
			.iter()
			.position(|candidate| &candidate.id == source)
	}

	fn symbol_by_id(&self, id: &SymbolId) -> Option<&SymbolRecord> {
		self.snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| &symbol.id == id)
	}

	fn moniker_for_identity(&self, identity: &str) -> Option<Moniker> {
		from_uri(
			identity,
			&UriConfig {
				scheme: &self.snapshot.index.identity_scheme,
			},
		)
		.ok()
	}

	fn compact_identity(&self, identity: &str) -> String {
		self.moniker_for_identity(identity)
			.as_ref()
			.map(compact_moniker)
			.unwrap_or_else(|| identity.to_string())
	}

	fn sort_defs_for_navigation(&self, locs: &mut [DefLocation]) {
		locs.sort_by(|left, right| self.compare_defs_for_navigation(left, right));
	}
}

impl IndexStore for SessionStoreBridge {
	fn root(&self) -> &str {
		&self.root
	}

	fn stats(&self) -> &SessionStats {
		&self.stats
	}

	fn linkage_stats(&self) -> &LinkageStats {
		&self.linkage_stats
	}

	fn file_count(&self) -> usize {
		self.snapshot.index.sources.len()
	}

	fn file_summary(&self, file_idx: usize) -> FileSummary {
		let source = self.source_file(file_idx);
		FileSummary {
			index: file_idx,
			lang: self.source_lang(source),
			rel_path: PathBuf::from(&source.rel_path),
			anchor: PathBuf::from(&source.anchor),
		}
	}

	fn all_navigable_defs(&self) -> Vec<DefLocation> {
		let mut out = self
			.snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.navigable)
			.filter_map(|symbol| self.symbol_loc(&symbol.id))
			.collect::<Vec<_>>();
		out.sort_by(|left, right| {
			self.symbol_for_loc(left)
				.identity
				.cmp(&self.symbol_for_loc(right).identity)
		});
		out
	}

	fn root_defs(&self, file_idx: usize) -> Vec<DefLocation> {
		let mut roots = self
			.snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.parent.is_none())
			.filter_map(|symbol| self.symbol_loc(&symbol.id))
			.filter(|loc| loc.file == file_idx)
			.collect::<Vec<_>>();
		self.sort_defs_for_navigation(&mut roots);
		roots
	}

	fn child_defs(&self, parent: &DefLocation) -> Vec<DefLocation> {
		let parent = self.symbol_for_loc(parent);
		self.children_for_symbol(&parent.id)
	}

	fn compare_defs_for_navigation(&self, left: &DefLocation, right: &DefLocation) -> Ordering {
		let left_symbol = self.symbol_for_loc(left);
		let right_symbol = self.symbol_for_loc(right);
		definition_kind_order(self.symbol_lang(left_symbol), &left_symbol.kind)
			.cmp(&definition_kind_order(
				self.symbol_lang(right_symbol),
				&right_symbol.kind,
			))
			.then_with(|| left_symbol.line_range.cmp(&right_symbol.line_range))
			.then_with(|| left_symbol.name.cmp(&right_symbol.name))
	}

	fn is_navigable_symbol(&self, loc: &DefLocation) -> bool {
		self.symbol_for_loc(loc).navigable
	}

	fn symbol_summary(&self, loc: &DefLocation) -> SymbolSummary {
		self.symbol_summary_for_record(self.symbol_for_loc(loc))
	}

	fn symbol_detail(&self, loc: &DefLocation) -> SymbolDetail {
		SymbolDetail {
			symbol: self.symbol_summary(loc),
			children: self
				.child_defs(loc)
				.iter()
				.map(|child| self.symbol_summary(child))
				.collect(),
		}
	}

	fn symbol_references(&self, loc: &DefLocation) -> SymbolReferences {
		let symbol = self.symbol_for_loc(loc);
		let incoming = self.incoming_refs_for_symbol(&symbol.id);
		let outgoing = self.outgoing_refs_for_symbol(&symbol.id);
		SymbolReferences {
			symbol: self.symbol_summary_for_record(symbol),
			incoming: self.reference_set(&incoming, ReferenceDirection::Incoming),
			outgoing: self.reference_set(&outgoing, ReferenceDirection::Outgoing),
		}
	}

	fn source_snippet(&self, loc: &DefLocation, context: u32) -> Vec<SourceLine> {
		let symbol = self.symbol_for_loc(loc);
		let Some((start_line, end_line)) = symbol.line_range else {
			return Vec::new();
		};
		let source = self.source_file(loc.file);
		let first = start_line.saturating_sub(context).max(1);
		let last = end_line.saturating_add(context);
		source
			.text
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
		langs: &[Lang],
		kinds: &[String],
		shapes: &[Shape],
	) -> Vec<SearchHit> {
		search_symbols(self, query, limit, langs, kinds, shapes)
	}

	fn change_overview(&self) -> ChangeOverview {
		let changed_files = self
			.snapshot
			.changes
			.changes
			.iter()
			.map(|change| change.file_path.clone())
			.collect::<BTreeSet<_>>()
			.len();
		ChangeOverview {
			scope: self.snapshot.changes.scope.clone(),
			change_count: self.snapshot.changes.changes.len(),
			file_count: changed_files,
			resources: self
				.snapshot
				.changes
				.resources
				.iter()
				.map(|resource| GitResourceSummary {
					available: resource.available,
					label: resource.label.clone(),
					message: resource.message.clone(),
				})
				.collect(),
			diagnostics: self.snapshot.changes.diagnostics.clone(),
		}
	}

	fn change_rows(&self) -> Vec<ChangeSummary> {
		self.snapshot
			.changes
			.changes
			.iter()
			.enumerate()
			.map(|(idx, change)| self.change_summary_for_record(idx, change))
			.collect()
	}

	fn change_summary(&self, change: LegacyChangeId) -> Option<ChangeSummary> {
		self.snapshot
			.changes
			.changes
			.get(change.index())
			.map(|record| self.change_summary_for_record(change.index(), record))
	}

	fn change_detail(&self, change: LegacyChangeId) -> Option<ChangeDetail> {
		self.snapshot
			.changes
			.changes
			.get(change.index())
			.map(|record| self.change_detail_for_record(change.index(), record))
	}

	fn changed_defs(&self) -> Vec<DefLocation> {
		self.snapshot
			.changes
			.changed_symbols
			.iter()
			.filter_map(|symbol| self.symbol_loc(symbol))
			.collect()
	}

	fn change_detail_for_symbol(&self, loc: &DefLocation) -> Option<ChangeDetail> {
		let symbol = self.symbol_id_for_loc(loc)?;
		self.snapshot
			.changes
			.changes
			.iter()
			.enumerate()
			.find(|(_, change)| change.symbol.as_ref() == Some(symbol))
			.map(|(idx, change)| self.change_detail_for_record(idx, change))
	}

	fn change_count_for_file(&self, file_idx: usize) -> usize {
		self.snapshot
			.changes
			.changes
			.iter()
			.filter(|change| {
				change
					.source
					.as_ref()
					.and_then(|source| self.source_file_index_by_id(source))
					.is_some_and(|source_idx| source_idx == file_idx)
			})
			.count()
	}

	fn usage_focus(&self, loc: DefLocation) -> UsageFocus {
		let symbol = self.symbol_for_loc(&loc);
		let target = self
			.moniker_for_identity(&symbol.identity)
			.unwrap_or_else(|| Moniker::from_canonical_bytes(symbol.identity.as_bytes().to_vec()));
		let refs = self.incoming_refs_for_symbol(&symbol.id);
		UsageFocus {
			label: symbol.name.clone(),
			compact_moniker: compact_moniker(&target),
			target,
			contexts: self.usage_contexts(&refs),
			references: self.reference_set(&refs, ReferenceDirection::Incoming),
			refs,
		}
	}

	fn unresolved_linkage_report(
		&self,
		file_limit: usize,
		samples_per_file: usize,
	) -> UnresolvedLinkageReport {
		let mut groups_by_file = FxHashMap::<usize, UnresolvedLinkageGroup>::default();
		for unresolved in &self.snapshot.linkage.unresolved {
			let Some(loc) = self.reference_loc(&unresolved.reference) else {
				continue;
			};
			self.push_unresolved_ref(&mut groups_by_file, &loc, samples_per_file);
		}
		let mut groups = groups_by_file.into_values().collect::<Vec<_>>();
		groups.sort_by(|left, right| {
			right
				.unresolved_refs
				.cmp(&left.unresolved_refs)
				.then_with(|| left.lang.tag().cmp(right.lang.tag()))
				.then_with(|| left.file_path.cmp(&right.file_path))
		});
		let files = groups.len();
		groups.truncate(file_limit);
		UnresolvedLinkageReport {
			unresolved_refs: self.snapshot.linkage.unresolved_refs,
			manifest_blocked_refs: self.snapshot.linkage.manifest_blocked_refs,
			files,
			shown_files: groups.len(),
			groups,
		}
	}

	fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary> {
		let cache = self
			.cache
			.clone()
			.ok_or_else(|| anyhow::anyhow!("bridge snapshot has no check runner context"))?;
		let mut runner = WorkspaceCheckRunner::new(
			WorkspaceCheckRunnerOptions::new(
				rules.to_path_buf(),
				profile.map(ToOwned::to_owned),
				scheme,
			),
			cache,
		);
		let diagnostics = runner
			.run_check(&self.snapshot.index, &self.snapshot.linkage)
			.map_err(|failure| anyhow::anyhow!(failure.to_string()))?;
		let files_with_violations = diagnostics
			.diagnostics
			.iter()
			.filter_map(|diagnostic| diagnostic.symbol.as_ref())
			.filter_map(|symbol| self.symbol_loc(symbol))
			.map(|loc| loc.file)
			.collect::<BTreeSet<_>>()
			.len();
		Ok(CheckSummary {
			files_scanned: self.snapshot.index.sources.len(),
			files_with_violations,
			total_violations: diagnostics.diagnostics.len(),
			errors: Vec::new(),
		})
	}
}

impl SessionStoreBridge {
	fn push_unresolved_ref(
		&self,
		groups_by_file: &mut FxHashMap<usize, UnresolvedLinkageGroup>,
		loc: &RefLocation,
		samples_per_file: usize,
	) {
		let source = self.source_file(loc.file);
		let entry = groups_by_file
			.entry(loc.file)
			.or_insert_with(|| UnresolvedLinkageGroup {
				lang: self.source_lang(source),
				file_path: PathBuf::from(&source.rel_path),
				unresolved_refs: 0,
				manifest_blocked_refs: 0,
				samples: Vec::new(),
			});
		entry.unresolved_refs += 1;
		if entry.samples.len() < samples_per_file {
			entry.samples.push(self.unresolved_sample(loc));
		}
	}

	fn unresolved_sample(&self, loc: &RefLocation) -> UnresolvedLinkageSample {
		let reference = self.reference_for_loc(loc);
		let source = self.symbol_by_id(&reference.source_symbol);
		UnresolvedLinkageSample {
			reason: "unresolved",
			kind: reference.kind.clone(),
			target: self.compact_identity(&reference.target_identity),
			source: source
				.map(|symbol| symbol.name.clone())
				.unwrap_or_else(|| reference.source_symbol.as_str().to_string()),
			location: self.reference_location(loc),
		}
	}
}

fn search_symbols(
	store: &SessionStoreBridge,
	query: &str,
	limit: usize,
	langs: &[Lang],
	kinds: &[String],
	shapes: &[Shape],
) -> Vec<SearchHit> {
	let raw = query.trim().to_ascii_lowercase();
	let terms = search_terms(&raw);
	if raw.is_empty() || terms.is_empty() || limit == 0 {
		return Vec::new();
	}
	let mut hits = store
		.snapshot
		.index
		.symbols
		.iter()
		.filter(|symbol| {
			symbol.navigable && search_filters_match(store, symbol, langs, kinds, shapes)
		})
		.filter_map(|symbol| {
			let loc = store.symbol_loc(&symbol.id)?;
			let (score, reason) = score_symbol(store, symbol, &raw, &terms)?;
			Some(SearchHit { loc, score, reason })
		})
		.collect::<Vec<_>>();
	hits.sort_by(|left, right| {
		right.score.cmp(&left.score).then_with(|| {
			store
				.symbol_for_loc(&left.loc)
				.identity
				.cmp(&store.symbol_for_loc(&right.loc).identity)
		})
	});
	hits.truncate(limit);
	hits
}

fn search_filters_match(
	store: &SessionStoreBridge,
	symbol: &SymbolRecord,
	langs: &[Lang],
	kinds: &[String],
	shapes: &[Shape],
) -> bool {
	let lang_matches = langs.is_empty() || langs.contains(&store.symbol_lang(symbol));
	let has_kind_filter = !kinds.is_empty() || !shapes.is_empty();
	let kind_matches = !kinds.is_empty() && kinds.iter().any(|filter| filter == &symbol.kind);
	let shape_matches = !shapes.is_empty()
		&& shape_of(symbol.kind.as_bytes()).is_some_and(|shape| shapes.contains(&shape));
	lang_matches && (!has_kind_filter || kind_matches || shape_matches)
}

fn score_symbol(
	store: &SessionStoreBridge,
	symbol: &SymbolRecord,
	phrase: &str,
	terms: &[String],
) -> Option<(u32, String)> {
	let name = symbol.name.to_ascii_lowercase();
	let kind = symbol.kind.to_ascii_lowercase();
	let path = store
		.symbol_source_path(symbol)
		.display()
		.to_string()
		.to_ascii_lowercase();
	let moniker = store
		.compact_identity(&symbol.identity)
		.to_ascii_lowercase();
	let signature = symbol.signature.to_ascii_lowercase();
	let fields = [
		("name", name.as_str(), 120, 50),
		("kind", kind.as_str(), 35, 20),
		("path", path.as_str(), 25, 12),
		("moniker", moniker.as_str(), 20, 10),
		("signature", signature.as_str(), 10, 5),
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

fn search_terms(query: &str) -> Vec<String> {
	query
		.split(|c: char| !c.is_alphanumeric())
		.filter(|term| !term.is_empty())
		.map(ToOwned::to_owned)
		.collect()
}

fn build_stats(snapshot: &WorkspaceSnapshot) -> SessionStats {
	let mut stats = SessionStats {
		files: snapshot.index.sources.len(),
		defs: snapshot.index.symbols.len(),
		refs: snapshot.index.references.len(),
		..SessionStats::default()
	};
	for source in &snapshot.index.sources {
		if let Some(lang) = Lang::from_tag(&source.language) {
			stats.by_lang.entry(lang.tag()).or_default().files += 1;
		}
	}
	for symbol in &snapshot.index.symbols {
		if let Some(source) = snapshot
			.index
			.sources
			.iter()
			.find(|source| source.id == symbol.source)
			&& let Some(lang) = Lang::from_tag(&source.language)
		{
			stats.by_lang.entry(lang.tag()).or_default().defs += 1;
		}
		*stats.by_def_kind.entry(symbol.kind.clone()).or_default() += 1;
		if let Some(shape) = shape_of(symbol.kind.as_bytes()) {
			*stats.by_shape.entry(shape.as_str()).or_default() += 1;
		}
	}
	for reference in &snapshot.index.references {
		if let Some(source) = snapshot
			.index
			.sources
			.iter()
			.find(|source| source.id == reference.source)
			&& let Some(lang) = Lang::from_tag(&source.language)
		{
			stats.by_lang.entry(lang.tag()).or_default().refs += 1;
		}
		*stats.by_ref_kind.entry(reference.kind.clone()).or_default() += 1;
	}
	stats
}

fn loc_for_symbol_id(id: &SymbolId) -> Option<DefLocation> {
	let mut parts = id.as_str().split(':');
	match (parts.next(), parts.next(), parts.next(), parts.next()) {
		(Some("symbol"), Some(file), Some(def), None) => Some(DefLocation {
			file: file.parse().ok()?,
			def: def.parse().ok()?,
		}),
		_ => None,
	}
}

fn loc_for_reference_id(id: &ReferenceId) -> Option<RefLocation> {
	let mut parts = id.as_str().split(':');
	match (parts.next(), parts.next(), parts.next(), parts.next()) {
		(Some("reference"), Some(file), Some(reference), None) => Some(RefLocation {
			file: file.parse().ok()?,
			reference: reference.parse().ok()?,
		}),
		_ => None,
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

fn legacy_change_status(status: ChangeStatus) -> LegacyChangeStatus {
	match status {
		ChangeStatus::Added => LegacyChangeStatus::Added,
		ChangeStatus::Modified => LegacyChangeStatus::Modified,
		ChangeStatus::Removed => LegacyChangeStatus::Removed,
	}
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
		Shape::Callable => 30,
		Shape::Value => 50,
		Shape::Annotation => 60,
		Shape::Ref => u16::MAX,
	}
}
