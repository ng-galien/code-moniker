// code-moniker: ignore-file[smell-feature-envy-local, smell-god-type-local-metrics, smell-large-type]
// TODO(smell): split workspace-backed TUI projections by panel/read-model once the old CLI workspace bridge deletion is complete.
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::shape::{Shape, shape_of};
use code_moniker_core::core::uri::{UriConfig, from_uri};
use code_moniker_core::lang::Lang;
use code_moniker_workspace::code::compact_moniker;
use code_moniker_workspace::facade::{
	LocalWorkspaceOptions, WorkspaceFacade, local_workspace_ports,
};
use code_moniker_workspace::snapshot::{
	ChangeId, ChangeStatus, ReferenceId, SourceFileRecord, SourceId, SymbolId, SymbolRecord,
	WorkspaceRequest, WorkspaceSnapshot, WorkspaceTransition,
};
use code_moniker_workspace::source::LocalResourceCache;
use rustc_hash::FxHashMap;

use crate::check::workspace::{WorkspaceCheckRunner, WorkspaceCheckRunnerOptions};
use crate::session::{
	CheckSummary, SessionOptions, SessionStats, StoreWatchRoot, watch_roots_for_options,
};

type LocalFacade = code_moniker_workspace::LocalWorkspaceFacade;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct LinkageStats {
	pub(in crate::ui) resolved_refs: usize,
	pub(in crate::ui) external_refs: usize,
	pub(in crate::ui) manifest_blocked_refs: usize,
	pub(in crate::ui) unresolved_refs: usize,
	pub(in crate::ui) ambiguous_refs: usize,
}

impl LinkageStats {
	pub(in crate::ui) fn eligible_refs(&self) -> usize {
		self.resolved_refs + self.manifest_blocked_refs + self.unresolved_refs
	}

	pub(in crate::ui) fn score_percent(&self) -> Option<u32> {
		let eligible = self.eligible_refs();
		(eligible > 0).then(|| ((self.resolved_refs * 100) / eligible) as u32)
	}
}

pub(in crate::ui) struct WorkspaceState {
	opts: SessionOptions,
	cache: LocalResourceCache,
	facade: LocalFacade,
	root: String,
	stats: SessionStats,
	linkage_stats: LinkageStats,
}

#[derive(Clone)]
pub(in crate::ui) struct WorkspaceCheckContext {
	cache: LocalResourceCache,
	snapshot: WorkspaceSnapshot,
}

impl WorkspaceCheckContext {
	pub(in crate::ui) fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary> {
		let mut runner = WorkspaceCheckRunner::new(
			WorkspaceCheckRunnerOptions::new(
				rules.to_path_buf(),
				profile.map(ToOwned::to_owned),
				scheme,
			),
			self.cache.clone(),
		);
		let diagnostics = runner
			.run_check(&self.snapshot.index, &self.snapshot.linkage)
			.map_err(|failure| anyhow::anyhow!(failure.to_string()))?;
		let files_with_violations = diagnostics
			.diagnostics
			.iter()
			.filter_map(|diagnostic| diagnostic.symbol.as_ref())
			.filter_map(|symbol| {
				self.snapshot
					.index
					.symbols
					.iter()
					.find(|candidate| &candidate.id == symbol)
			})
			.map(|symbol| symbol.source.clone())
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

impl WorkspaceState {
	pub(in crate::ui) fn empty(opts: SessionOptions) -> Self {
		Self::new_unloaded(opts)
	}

	pub(in crate::ui) fn catalog(opts: &SessionOptions) -> anyhow::Result<Self> {
		Self::load(opts)
	}

	pub(in crate::ui) fn load(opts: &SessionOptions) -> anyhow::Result<Self> {
		let mut state = Self::new_unloaded(opts.clone());
		state.refresh()?;
		Ok(state)
	}

	fn new_unloaded(opts: SessionOptions) -> Self {
		let cache = LocalResourceCache::default();
		let facade = WorkspaceFacade::new(local_workspace_ports(
			LocalWorkspaceOptions::new(opts.paths.clone(), opts.project.clone())
				.with_cache_dir(opts.cache_dir.clone()),
			cache.clone(),
		));
		let root = display_boot_path(&opts.paths);
		Self {
			opts,
			cache,
			facade,
			root,
			stats: SessionStats::default(),
			linkage_stats: LinkageStats::default(),
		}
	}

	pub(in crate::ui) fn refresh(&mut self) -> anyhow::Result<()> {
		match self.facade.refresh(WorkspaceRequest::new("workspace")) {
			WorkspaceTransition::Ready { .. } => {
				self.recompute_stats();
				Ok(())
			}
			WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
		}
	}

	pub(in crate::ui) fn reload(&mut self) -> anyhow::Result<()> {
		*self = Self::load(&self.opts)?;
		Ok(())
	}

	pub(in crate::ui) fn options(&self) -> SessionOptions {
		self.opts.clone()
	}

	pub(in crate::ui) fn watch_roots(&self) -> Vec<StoreWatchRoot> {
		watch_roots_for_options(&self.opts)
	}

	pub(in crate::ui) fn refresh_git_overlay(&mut self) {
		let _ = self.reload();
	}

	pub(in crate::ui) fn root(&self) -> &str {
		&self.root
	}

	pub(in crate::ui) fn stats(&self) -> &SessionStats {
		&self.stats
	}

	pub(in crate::ui) fn linkage_stats(&self) -> &LinkageStats {
		&self.linkage_stats
	}

	pub(in crate::ui) fn file_count(&self) -> usize {
		self.snapshot()
			.map_or(0, |snapshot| snapshot.index.sources.len())
	}

	pub(in crate::ui) fn file_summary(&self, file_idx: usize) -> FileSummary {
		let source = self.source_file(file_idx);
		FileSummary {
			index: file_idx,
			lang: source
				.and_then(|source| Lang::from_tag(&source.language))
				.unwrap_or(Lang::Rs),
			rel_path: source
				.map(|source| PathBuf::from(&source.rel_path))
				.unwrap_or_default(),
			anchor: source
				.map(|source| PathBuf::from(&source.anchor))
				.unwrap_or_default(),
		}
	}

	pub(in crate::ui) fn all_navigable_defs(&self) -> Vec<SymbolId> {
		let Some(snapshot) = self.snapshot() else {
			return Vec::new();
		};
		snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.navigable)
			.map(|symbol| symbol.id.clone())
			.collect()
	}

	pub(in crate::ui) fn root_defs(&self, file_idx: usize) -> Vec<SymbolId> {
		let Some(source) = self.source_file(file_idx) else {
			return Vec::new();
		};
		let mut roots = self
			.snapshot()
			.into_iter()
			.flat_map(|snapshot| &snapshot.index.symbols)
			.filter(|symbol| {
				symbol.navigable && symbol.source == source.id && symbol.parent.is_none()
			})
			.map(|symbol| symbol.id.clone())
			.collect::<Vec<_>>();
		self.sort_defs_for_navigation(&mut roots);
		roots
	}

	pub(in crate::ui) fn child_defs(&self, parent: &SymbolId) -> Vec<SymbolId> {
		let mut children = self
			.snapshot()
			.into_iter()
			.flat_map(|snapshot| &snapshot.index.symbols)
			.filter(|symbol| symbol.navigable && symbol.parent.as_ref() == Some(parent))
			.map(|symbol| symbol.id.clone())
			.collect::<Vec<_>>();
		self.sort_defs_for_navigation(&mut children);
		children
	}

	pub(in crate::ui) fn compare_defs_for_navigation(
		&self,
		left: &SymbolId,
		right: &SymbolId,
	) -> Ordering {
		let left_symbol = self.symbol_by_id(left);
		let right_symbol = self.symbol_by_id(right);
		match (left_symbol, right_symbol) {
			(Some(left), Some(right)) => definition_kind_order(self.symbol_lang(left), &left.kind)
				.cmp(&definition_kind_order(self.symbol_lang(right), &right.kind))
				.then_with(|| left.line_range.cmp(&right.line_range))
				.then_with(|| left.name.cmp(&right.name)),
			_ => left.as_str().cmp(right.as_str()),
		}
	}

	pub(in crate::ui) fn is_navigable_symbol(&self, symbol: &SymbolId) -> bool {
		self.symbol_by_id(symbol)
			.is_some_and(|symbol| symbol.navigable)
	}

	pub(in crate::ui) fn symbol_summary(&self, symbol: &SymbolId) -> SymbolSummary {
		let Some(record) = self.symbol_by_id(symbol) else {
			return SymbolSummary::missing(symbol.clone());
		};
		SymbolSummary {
			id: record.id.clone(),
			lang: self.symbol_lang(record),
			kind: record.kind.clone(),
			name: record.name.clone(),
			file_path: self.symbol_source_path(record),
			compact_moniker: self.compact_identity(&record.identity),
			line_range: record.line_range,
			child_count: self.child_defs(&record.id).len(),
			change: self.change_badge_for_symbol(&record.id),
		}
	}

	pub(in crate::ui) fn symbol_detail(&self, symbol: &SymbolId) -> SymbolDetail {
		SymbolDetail {
			symbol: self.symbol_summary(symbol),
			children: self
				.child_defs(symbol)
				.iter()
				.map(|child| self.symbol_summary(child))
				.collect(),
		}
	}

	pub(in crate::ui) fn symbol_references(&self, symbol: &SymbolId) -> SymbolReferences {
		SymbolReferences {
			symbol: self.symbol_summary(symbol),
			incoming: self.reference_set(&self.incoming_refs_for_symbol(symbol), "source"),
			outgoing: self.reference_set(&self.outgoing_refs_for_symbol(symbol), "target"),
		}
	}

	pub(in crate::ui) fn source_snippet(&self, symbol: &SymbolId, context: u32) -> Vec<SourceLine> {
		let Some(record) = self.symbol_by_id(symbol) else {
			return Vec::new();
		};
		let Some((start_line, end_line)) = record.line_range else {
			return Vec::new();
		};
		let Some(source) = self.source_file_by_id(&record.source) else {
			return Vec::new();
		};
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

	pub(in crate::ui) fn search_symbols_filtered(
		&self,
		query: &str,
		limit: usize,
		langs: &[Lang],
		kinds: &[String],
		shapes: &[Shape],
	) -> Vec<SearchHit> {
		let mut hits = self
			.facade
			.view()
			.map(|view| view.search().search_symbols(query, limit.saturating_mul(4)))
			.unwrap_or_default()
			.into_iter()
			.filter(|hit| {
				self.symbol_by_id(&hit.symbol)
					.is_some_and(|symbol| search_filters_match(self, symbol, langs, kinds, shapes))
			})
			.map(|hit| SearchHit {
				loc: hit.symbol,
				score: hit.score,
				reason: hit.reason,
			})
			.collect::<Vec<_>>();
		hits.truncate(limit);
		hits
	}

	pub(in crate::ui) fn change_overview(&self) -> ChangeOverview {
		let Some(snapshot) = self.snapshot() else {
			return ChangeOverview::default();
		};
		let changed_files = snapshot
			.changes
			.changes
			.iter()
			.map(|change| change.file_path.clone())
			.collect::<BTreeSet<_>>()
			.len();
		ChangeOverview {
			scope: snapshot.changes.scope.clone(),
			change_count: snapshot.changes.changes.len(),
			file_count: changed_files,
			resources: snapshot
				.changes
				.resources
				.iter()
				.map(|resource| GitResourceSummary {
					available: resource.available,
					label: resource.label.clone(),
					message: resource.message.clone(),
				})
				.collect(),
			diagnostics: snapshot.changes.diagnostics.clone(),
		}
	}

	pub(in crate::ui) fn change_rows(&self) -> Vec<ChangeSummary> {
		self.facade
			.view()
			.map(|view| {
				view.changes()
					.summaries()
					.into_iter()
					.map(|summary| self.change_summary_from_view(summary))
					.collect()
			})
			.unwrap_or_default()
	}

	pub(in crate::ui) fn change_summary(&self, change: ChangeId) -> Option<ChangeSummary> {
		self.facade
			.view()?
			.changes()
			.detail(&change)
			.map(|detail| self.change_summary_from_view(detail.summary))
	}

	pub(in crate::ui) fn change_detail(&self, change: ChangeId) -> Option<ChangeDetail> {
		let detail = self.facade.view()?.changes().detail(&change)?;
		Some(ChangeDetail {
			summary: self.change_summary_from_view(detail.summary),
			blast_radius: self.reference_set_from_view(detail.blast_radius),
		})
	}

	pub(in crate::ui) fn changed_defs(&self) -> Vec<SymbolId> {
		self.snapshot()
			.map(|snapshot| snapshot.changes.changed_symbols.clone())
			.unwrap_or_default()
	}

	pub(in crate::ui) fn change_detail_for_symbol(
		&self,
		symbol: &SymbolId,
	) -> Option<ChangeDetail> {
		let change = self
			.snapshot()?
			.changes
			.changes
			.iter()
			.find(|change| change.symbol.as_ref() == Some(symbol))?
			.id
			.clone();
		self.change_detail(change)
	}

	pub(in crate::ui) fn change_count_for_file(&self, file_idx: usize) -> usize {
		let Some(source) = self.source_file(file_idx) else {
			return 0;
		};
		self.snapshot()
			.map(|snapshot| {
				snapshot
					.changes
					.changes
					.iter()
					.filter(|change| change.source.as_ref() == Some(&source.id))
					.count()
			})
			.unwrap_or(0)
	}

	pub(in crate::ui) fn usage_focus(&self, symbol: SymbolId) -> Option<UsageFocus> {
		let record = self.symbol_by_id(&symbol)?;
		let target = self
			.moniker_for_identity(&record.identity)
			.unwrap_or_else(|| Moniker::from_canonical_bytes(record.identity.as_bytes().to_vec()));
		let refs = self.incoming_refs_for_symbol(&symbol);
		Some(UsageFocus {
			target,
			label: record.name.clone(),
			compact_moniker: self.compact_identity(&record.identity),
			refs: refs.clone(),
			contexts: self.usage_contexts(&refs),
			references: self.reference_set(&refs, "source"),
		})
	}

	pub(in crate::ui) fn usage_focus_for_target(
		&self,
		target: Moniker,
		label: String,
	) -> Option<UsageFocus> {
		let symbol = self
			.snapshot()?
			.index
			.symbols
			.iter()
			.find(|symbol| self.moniker_for_identity(&symbol.identity).as_ref() == Some(&target))
			.map(|symbol| symbol.id.clone())?;
		let mut focus = self.usage_focus(symbol)?;
		focus.target = target;
		focus.label = label;
		Some(focus)
	}

	pub(in crate::ui) fn unresolved_linkage_report(
		&self,
		file_limit: usize,
		samples_per_file: usize,
	) -> UnresolvedLinkageReport {
		let Some(snapshot) = self.snapshot() else {
			return UnresolvedLinkageReport::default();
		};
		let mut groups_by_file = FxHashMap::<SourceId, UnresolvedLinkageGroup>::default();
		for unresolved in &snapshot.linkage.unresolved {
			let Some(reference) = self.reference_by_id(&unresolved.reference) else {
				continue;
			};
			let Some(source) = self.source_file_by_id(&reference.source) else {
				continue;
			};
			let entry = groups_by_file
				.entry(reference.source.clone())
				.or_insert_with(|| UnresolvedLinkageGroup {
					lang: Lang::from_tag(&source.language).unwrap_or(Lang::Rs),
					file_path: PathBuf::from(&source.rel_path),
					unresolved_refs: 0,
					manifest_blocked_refs: 0,
					samples: Vec::new(),
				});
			entry.unresolved_refs += 1;
			if entry.samples.len() < samples_per_file {
				entry.samples.push(UnresolvedLinkageSample {
					reason: "unresolved",
					kind: reference.kind.clone(),
					target: self.compact_identity(&reference.target_identity),
					source: self
						.symbol_by_id(&reference.source_symbol)
						.map(|symbol| symbol.name.clone())
						.unwrap_or_else(|| reference.source_symbol.as_str().to_string()),
					location: self.reference_location(reference),
				});
			}
		}
		let mut groups = groups_by_file.into_values().collect::<Vec<_>>();
		groups.sort_by(|left, right| {
			right
				.unresolved_refs
				.cmp(&left.unresolved_refs)
				.then_with(|| left.file_path.cmp(&right.file_path))
		});
		let files = groups.len();
		groups.truncate(file_limit);
		UnresolvedLinkageReport {
			unresolved_refs: snapshot.linkage.unresolved_refs,
			manifest_blocked_refs: snapshot.linkage.manifest_blocked_refs,
			files,
			shown_files: groups.len(),
			groups,
		}
	}

	pub(in crate::ui) fn check_summary(
		&self,
		rules: &Path,
		profile: Option<&str>,
		scheme: &str,
	) -> anyhow::Result<CheckSummary> {
		self.check_context()?.check_summary(rules, profile, scheme)
	}

	pub(in crate::ui) fn check_context(&self) -> anyhow::Result<WorkspaceCheckContext> {
		Ok(WorkspaceCheckContext {
			cache: self.cache.clone(),
			snapshot: self
				.snapshot()
				.ok_or_else(|| anyhow::anyhow!("workspace snapshot is unavailable"))?
				.clone(),
		})
	}

	fn snapshot(&self) -> Option<&WorkspaceSnapshot> {
		self.facade.snapshot()
	}

	fn recompute_stats(&mut self) {
		let Some(snapshot) = self.snapshot() else {
			self.stats = SessionStats::default();
			self.linkage_stats = LinkageStats::default();
			return;
		};
		let stats = build_stats(snapshot);
		let linkage_stats = LinkageStats {
			resolved_refs: snapshot.linkage.resolved_refs,
			external_refs: snapshot.linkage.external_refs,
			manifest_blocked_refs: snapshot.linkage.manifest_blocked_refs,
			unresolved_refs: snapshot.linkage.unresolved_refs,
			ambiguous_refs: snapshot.linkage.ambiguous_refs,
		};
		self.stats = stats;
		self.linkage_stats = LinkageStats { ..linkage_stats };
	}

	fn source_file(&self, file_idx: usize) -> Option<&SourceFileRecord> {
		self.snapshot()?.index.sources.get(file_idx)
	}

	fn source_file_by_id(&self, source: &SourceId) -> Option<&SourceFileRecord> {
		self.snapshot()?
			.index
			.sources
			.iter()
			.find(|candidate| &candidate.id == source)
	}

	fn symbol_by_id(&self, id: &SymbolId) -> Option<&SymbolRecord> {
		self.snapshot()?
			.index
			.symbols
			.iter()
			.find(|symbol| &symbol.id == id)
	}

	fn reference_by_id(
		&self,
		id: &ReferenceId,
	) -> Option<&code_moniker_workspace::snapshot::ReferenceRecord> {
		self.snapshot()?
			.index
			.references
			.iter()
			.find(|reference| &reference.id == id)
	}

	fn symbol_lang(&self, symbol: &SymbolRecord) -> Lang {
		self.source_file_by_id(&symbol.source)
			.and_then(|source| Lang::from_tag(&source.language))
			.unwrap_or(Lang::Rs)
	}

	fn symbol_source_path(&self, symbol: &SymbolRecord) -> PathBuf {
		self.source_file_by_id(&symbol.source)
			.map(|source| PathBuf::from(&source.rel_path))
			.unwrap_or_default()
	}

	fn incoming_refs_for_symbol(&self, symbol: &SymbolId) -> Vec<ReferenceId> {
		self.snapshot()
			.map(|snapshot| {
				snapshot
					.linkage
					.resolved
					.iter()
					.filter(|edge| &edge.target == symbol)
					.map(|edge| edge.reference.clone())
					.collect()
			})
			.unwrap_or_default()
	}

	fn outgoing_refs_for_symbol(&self, symbol: &SymbolId) -> Vec<ReferenceId> {
		self.snapshot()
			.map(|snapshot| {
				snapshot
					.index
					.references
					.iter()
					.filter(|reference| &reference.source_symbol == symbol)
					.map(|reference| reference.id.clone())
					.collect()
			})
			.unwrap_or_default()
	}

	fn reference_set(&self, refs: &[ReferenceId], endpoint_label: &'static str) -> ReferenceSet {
		let groups = refs
			.iter()
			.filter_map(|id| self.reference_by_id(id))
			.map(|reference| self.reference_group(reference, endpoint_label))
			.collect::<Vec<_>>();
		let files = groups
			.iter()
			.map(|group| group.location.clone())
			.collect::<BTreeSet<_>>()
			.len();
		let contexts = self.usage_contexts(refs).len();
		ReferenceSet {
			summary: ReferenceSetSummary {
				refs: groups.len(),
				files,
				contexts,
			},
			groups,
		}
	}

	fn reference_set_from_view(
		&self,
		refs: code_moniker_workspace::snapshot::ReferenceSet,
	) -> ReferenceSet {
		let ids = refs
			.groups
			.into_iter()
			.map(|reference| reference.reference)
			.collect::<Vec<_>>();
		self.reference_set(&ids, "source")
	}

	fn reference_group(
		&self,
		reference: &code_moniker_workspace::snapshot::ReferenceRecord,
		endpoint_label: &'static str,
	) -> ReferenceGroup {
		let source = self.symbol_by_id(&reference.source_symbol);
		ReferenceGroup {
			kinds: vec![reference.kind.clone()],
			actor: source
				.map(|symbol| symbol.name.clone())
				.unwrap_or_else(|| reference.source_symbol.as_str().to_string()),
			location: self.reference_location(reference),
			endpoint_label,
			endpoint: self.compact_identity(&reference.target_identity),
			confidence: reference
				.confidence
				.clone()
				.unwrap_or_else(|| "-".to_string()),
			receiver: reference.receiver.clone(),
			alias: reference.alias.clone(),
		}
	}

	fn reference_location(
		&self,
		reference: &code_moniker_workspace::snapshot::ReferenceRecord,
	) -> String {
		let path = self
			.source_file_by_id(&reference.source)
			.map(|source| source.rel_path.as_str())
			.unwrap_or("<source>");
		let lines = reference
			.line_range
			.map(|(start, end)| {
				if start == end {
					format!("L{start}")
				} else {
					format!("L{start}-L{end}")
				}
			})
			.unwrap_or_else(|| "L?".to_string());
		format!("{path}:{lines}")
	}

	fn usage_contexts(&self, refs: &[ReferenceId]) -> Vec<SymbolId> {
		refs.iter()
			.filter_map(|id| self.reference_by_id(id))
			.map(|reference| reference.source_symbol.clone())
			.fold(Vec::new(), |mut out, symbol| {
				if !out.contains(&symbol) {
					out.push(symbol);
				}
				out
			})
	}

	fn change_badge_for_symbol(&self, symbol: &SymbolId) -> Option<ChangeBadge> {
		let change = self
			.snapshot()?
			.changes
			.changes
			.iter()
			.find(|change| change.symbol.as_ref() == Some(symbol))?;
		Some(ChangeBadge {
			status: change.status,
			usage_count: self.incoming_refs_for_symbol(symbol).len(),
		})
	}

	fn change_summary_from_view(
		&self,
		summary: code_moniker_workspace::snapshot::ChangeSummary,
	) -> ChangeSummary {
		let source = summary
			.source
			.as_ref()
			.and_then(|id| self.source_file_by_id(id));
		ChangeSummary {
			id: summary.id,
			status: summary.status,
			lang: source
				.and_then(|source| Lang::from_tag(&source.language))
				.unwrap_or(Lang::Rs),
			kind: summary.kind,
			name: summary.name,
			file_path: source
				.map(|source| PathBuf::from(&source.rel_path))
				.unwrap_or_default(),
			compact_moniker: self.compact_identity(&summary.identity),
			line_range: summary.line_range,
			hunk_count: summary.hunk_count,
			usage_count: summary.usage_count,
		}
	}

	fn moniker_for_identity(&self, identity: &str) -> Option<Moniker> {
		from_uri(
			identity,
			&UriConfig {
				scheme: self
					.snapshot()
					.map(|snapshot| snapshot.index.identity_scheme.as_str())
					.unwrap_or(crate::DEFAULT_SCHEME),
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

	fn sort_defs_for_navigation(&self, symbols: &mut [SymbolId]) {
		symbols.sort_by(|left, right| self.compare_defs_for_navigation(left, right));
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct FileSummary {
	pub(in crate::ui) index: usize,
	pub(in crate::ui) lang: Lang,
	pub(in crate::ui) rel_path: PathBuf,
	pub(in crate::ui) anchor: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct SymbolSummary {
	pub(in crate::ui) id: SymbolId,
	pub(in crate::ui) lang: Lang,
	pub(in crate::ui) kind: String,
	pub(in crate::ui) name: String,
	pub(in crate::ui) file_path: PathBuf,
	pub(in crate::ui) compact_moniker: String,
	pub(in crate::ui) line_range: Option<(u32, u32)>,
	pub(in crate::ui) child_count: usize,
	pub(in crate::ui) change: Option<ChangeBadge>,
}

impl SymbolSummary {
	fn missing(id: SymbolId) -> Self {
		Self {
			id,
			lang: Lang::Rs,
			kind: String::new(),
			name: "<missing>".to_string(),
			file_path: PathBuf::new(),
			compact_moniker: String::new(),
			line_range: None,
			child_count: 0,
			change: None,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct SymbolDetail {
	pub(in crate::ui) symbol: SymbolSummary,
	pub(in crate::ui) children: Vec<SymbolSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ChangeBadge {
	pub(in crate::ui) status: ChangeStatus,
	pub(in crate::ui) usage_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ChangeSummary {
	pub(in crate::ui) id: ChangeId,
	pub(in crate::ui) status: ChangeStatus,
	pub(in crate::ui) lang: Lang,
	pub(in crate::ui) kind: String,
	pub(in crate::ui) name: String,
	pub(in crate::ui) file_path: PathBuf,
	pub(in crate::ui) compact_moniker: String,
	pub(in crate::ui) line_range: Option<(u32, u32)>,
	pub(in crate::ui) hunk_count: usize,
	pub(in crate::ui) usage_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ChangeDetail {
	pub(in crate::ui) summary: ChangeSummary,
	pub(in crate::ui) blast_radius: ReferenceSet,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct ChangeOverview {
	pub(in crate::ui) scope: String,
	pub(in crate::ui) change_count: usize,
	pub(in crate::ui) file_count: usize,
	pub(in crate::ui) resources: Vec<GitResourceSummary>,
	pub(in crate::ui) diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct GitResourceSummary {
	pub(in crate::ui) available: bool,
	pub(in crate::ui) label: String,
	pub(in crate::ui) message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ReferenceSet {
	pub(in crate::ui) summary: ReferenceSetSummary,
	pub(in crate::ui) groups: Vec<ReferenceGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ReferenceSetSummary {
	pub(in crate::ui) refs: usize,
	pub(in crate::ui) files: usize,
	pub(in crate::ui) contexts: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ReferenceGroup {
	pub(in crate::ui) kinds: Vec<String>,
	pub(in crate::ui) actor: String,
	pub(in crate::ui) location: String,
	pub(in crate::ui) endpoint_label: &'static str,
	pub(in crate::ui) endpoint: String,
	pub(in crate::ui) confidence: String,
	pub(in crate::ui) receiver: Option<String>,
	pub(in crate::ui) alias: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct SymbolReferences {
	pub(in crate::ui) symbol: SymbolSummary,
	pub(in crate::ui) incoming: ReferenceSet,
	pub(in crate::ui) outgoing: ReferenceSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct UsageFocus {
	pub(in crate::ui) target: Moniker,
	pub(in crate::ui) label: String,
	pub(in crate::ui) compact_moniker: String,
	pub(in crate::ui) refs: Vec<ReferenceId>,
	pub(in crate::ui) contexts: Vec<SymbolId>,
	pub(in crate::ui) references: ReferenceSet,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct UnresolvedLinkageReport {
	pub(in crate::ui) unresolved_refs: usize,
	pub(in crate::ui) manifest_blocked_refs: usize,
	pub(in crate::ui) files: usize,
	pub(in crate::ui) shown_files: usize,
	pub(in crate::ui) groups: Vec<UnresolvedLinkageGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct UnresolvedLinkageGroup {
	pub(in crate::ui) lang: Lang,
	pub(in crate::ui) file_path: PathBuf,
	pub(in crate::ui) unresolved_refs: usize,
	pub(in crate::ui) manifest_blocked_refs: usize,
	pub(in crate::ui) samples: Vec<UnresolvedLinkageSample>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct UnresolvedLinkageSample {
	pub(in crate::ui) reason: &'static str,
	pub(in crate::ui) kind: String,
	pub(in crate::ui) target: String,
	pub(in crate::ui) source: String,
	pub(in crate::ui) location: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct SearchHit {
	pub(in crate::ui) loc: SymbolId,
	pub(in crate::ui) score: u32,
	pub(in crate::ui) reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct SourceLine {
	pub(in crate::ui) number: u32,
	pub(in crate::ui) text: String,
	pub(in crate::ui) active: bool,
}

fn search_filters_match(
	store: &WorkspaceState,
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
