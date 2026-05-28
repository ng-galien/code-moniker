use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
use rustc_hash::{FxHashMap, FxHashSet};

use crate::check::workspace::{WorkspaceCheckRunner, WorkspaceCheckRunnerOptions};
use crate::session::{CheckSummary, SessionOptions, SessionStats};

pub(in crate::ui) use code_moniker_workspace::LocalWorkspaceFacade;

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

#[derive(Clone)]
pub(in crate::ui) struct WorkspaceCheckContext {
	cache: LocalResourceCache,
	snapshot: Arc<WorkspaceSnapshot>,
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

pub(in crate::ui) fn new_local_workspace(
	opts: &SessionOptions,
) -> (
	code_moniker_workspace::LocalWorkspaceFacade,
	LocalResourceCache,
) {
	let cache = LocalResourceCache::default();
	new_local_workspace_with_cache(opts, cache)
}

fn new_local_workspace_with_cache(
	opts: &SessionOptions,
	cache: LocalResourceCache,
) -> (
	code_moniker_workspace::LocalWorkspaceFacade,
	LocalResourceCache,
) {
	let facade = WorkspaceFacade::new(local_workspace_ports(
		LocalWorkspaceOptions::new(opts.paths.clone(), opts.project.clone())
			.with_cache_dir(opts.cache_dir.clone()),
		cache.clone(),
	));
	(facade, cache)
}

#[cfg(test)]
pub(in crate::ui) fn load_local_workspace(
	opts: &SessionOptions,
) -> anyhow::Result<(
	code_moniker_workspace::LocalWorkspaceFacade,
	LocalResourceCache,
)> {
	let (mut facade, cache) = new_local_workspace(opts);
	refresh_workspace(&mut facade)?;
	Ok((facade, cache))
}

pub(in crate::ui) fn load_local_file_catalog(
	opts: &SessionOptions,
) -> anyhow::Result<(
	code_moniker_workspace::LocalWorkspaceFacade,
	LocalResourceCache,
)> {
	let (mut facade, cache) = new_local_workspace(opts);
	match facade.load_catalog(WorkspaceRequest::new("file-catalog")) {
		WorkspaceTransition::Ready { .. } => Ok((facade, cache)),
		WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
	}
}

pub(in crate::ui) fn load_local_symbol_index(
	opts: &SessionOptions,
) -> anyhow::Result<(
	code_moniker_workspace::LocalWorkspaceFacade,
	LocalResourceCache,
)> {
	let (mut facade, cache) = new_local_workspace(opts);
	match facade.load_index(WorkspaceRequest::new("symbol-index")) {
		WorkspaceTransition::Ready { .. } => Ok((facade, cache)),
		WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
	}
}

pub(in crate::ui) fn load_local_symbol_index_from_catalog(
	opts: &SessionOptions,
	cache: LocalResourceCache,
	snapshot: Arc<WorkspaceSnapshot>,
) -> anyhow::Result<(
	code_moniker_workspace::LocalWorkspaceFacade,
	LocalResourceCache,
)> {
	let (mut facade, cache) = new_local_workspace_with_cache(opts, cache);
	facade.replace_snapshot_arc(snapshot);
	match facade.load_index(WorkspaceRequest::new("symbol-index").reuse_current_catalog()) {
		WorkspaceTransition::Ready { .. } => Ok((facade, cache)),
		WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
	}
}

pub(in crate::ui) fn resolve_local_linkage(
	opts: &SessionOptions,
	cache: LocalResourceCache,
	snapshot: Arc<WorkspaceSnapshot>,
) -> anyhow::Result<(
	code_moniker_workspace::LocalWorkspaceFacade,
	LocalResourceCache,
)> {
	let (mut facade, cache) = new_local_workspace_with_cache(opts, cache);
	facade.replace_snapshot_arc(snapshot);
	match facade.resolve_linkage(WorkspaceRequest::new("linkage")) {
		WorkspaceTransition::Ready { .. } => Ok((facade, cache)),
		WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
	}
}

pub(in crate::ui) fn refresh_workspace(
	facade: &mut code_moniker_workspace::LocalWorkspaceFacade,
) -> anyhow::Result<()> {
	match facade.refresh(WorkspaceRequest::new("workspace")) {
		WorkspaceTransition::Ready { .. } => Ok(()),
		WorkspaceTransition::Failed { failure, .. } => anyhow::bail!(failure.message),
	}
}

pub(in crate::ui) fn workspace_check_context(
	facade: &code_moniker_workspace::LocalWorkspaceFacade,
	cache: &LocalResourceCache,
) -> anyhow::Result<WorkspaceCheckContext> {
	Ok(WorkspaceCheckContext {
		cache: cache.clone(),
		snapshot: facade
			.snapshot_arc()
			.ok_or_else(|| anyhow::anyhow!("workspace snapshot is unavailable"))?,
	})
}

pub(in crate::ui) fn stats(store: &LocalWorkspaceFacade) -> SessionStats {
	store.snapshot().map(build_stats).unwrap_or_default()
}

pub(in crate::ui) fn linkage_stats(store: &LocalWorkspaceFacade) -> LinkageStats {
	store
		.snapshot()
		.map(|snapshot| LinkageStats {
			resolved_refs: snapshot.linkage.resolved_refs,
			external_refs: snapshot.linkage.external_refs,
			manifest_blocked_refs: snapshot.linkage.manifest_blocked_refs,
			unresolved_refs: snapshot.linkage.unresolved_refs,
			ambiguous_refs: snapshot.linkage.ambiguous_refs,
		})
		.unwrap_or_default()
}

pub(in crate::ui) fn navigable_defs_filtered(
	store: &LocalWorkspaceFacade,
	langs: &[Lang],
	kinds: &[String],
	shapes: &[Shape],
) -> Vec<SymbolId> {
	let Some(snapshot) = store.snapshot() else {
		return Vec::new();
	};
	let source_langs = source_lang_index(snapshot);
	snapshot
		.index
		.symbols
		.iter()
		.filter(|symbol| {
			symbol.navigable && symbol_matches_filters(symbol, &source_langs, langs, kinds, shapes)
		})
		.map(|symbol| symbol.id.clone())
		.collect()
}

pub(in crate::ui) fn available_langs(store: &LocalWorkspaceFacade) -> Vec<Lang> {
	let Some(snapshot) = store.snapshot() else {
		return Vec::new();
	};
	let present = snapshot
		.index
		.sources
		.iter()
		.filter_map(|source| Lang::from_tag(&source.language))
		.collect::<FxHashSet<_>>();
	Lang::ALL
		.iter()
		.copied()
		.filter(|lang| present.contains(lang))
		.collect()
}

pub(in crate::ui) fn available_kinds_for_lang(
	store: &LocalWorkspaceFacade,
	lang: Lang,
) -> Vec<String> {
	let Some(snapshot) = store.snapshot() else {
		return Vec::new();
	};
	let source_langs = source_lang_index(snapshot);
	let mut kinds = BTreeSet::new();
	for symbol in &snapshot.index.symbols {
		if symbol.navigable && source_langs.get(&symbol.source) == Some(&lang) {
			kinds.insert(symbol.kind.clone());
		}
	}
	kinds.into_iter().collect()
}

pub(in crate::ui) fn available_shapes(store: &LocalWorkspaceFacade, langs: &[Lang]) -> Vec<Shape> {
	let Some(snapshot) = store.snapshot() else {
		return Vec::new();
	};
	let source_langs = source_lang_index(snapshot);
	let selected = langs.iter().copied().collect::<FxHashSet<_>>();
	let mut present = Vec::new();
	for symbol in &snapshot.index.symbols {
		if !symbol.navigable {
			continue;
		}
		let Some(lang) = source_langs.get(&symbol.source).copied() else {
			continue;
		};
		if !selected.is_empty() && !selected.contains(&lang) {
			continue;
		}
		if let Some(shape) = shape_of(symbol.kind.as_bytes())
			&& !present.contains(&shape)
		{
			present.push(shape);
		}
	}
	Shape::ALL
		.iter()
		.copied()
		.filter(|shape| present.contains(shape))
		.collect()
}

pub(in crate::ui) fn child_defs(store: &LocalWorkspaceFacade, parent: &SymbolId) -> Vec<SymbolId> {
	let mut children = store
		.snapshot()
		.into_iter()
		.flat_map(|snapshot| &snapshot.index.symbols)
		.filter(|symbol| symbol.navigable && symbol.parent.as_ref() == Some(parent))
		.map(|symbol| symbol.id.clone())
		.collect::<Vec<_>>();
	sort_defs_for_navigation(store, &mut children);
	children
}

pub(in crate::ui) fn compare_defs_for_navigation(
	store: &LocalWorkspaceFacade,
	left: &SymbolId,
	right: &SymbolId,
) -> Ordering {
	let left_symbol = symbol_by_id(store, left);
	let right_symbol = symbol_by_id(store, right);
	match (left_symbol, right_symbol) {
		(Some(left), Some(right)) => definition_kind_order(symbol_lang(store, left), &left.kind)
			.cmp(&definition_kind_order(
				symbol_lang(store, right),
				&right.kind,
			))
			.then_with(|| left.line_range.cmp(&right.line_range))
			.then_with(|| left.name.cmp(&right.name)),
		_ => left.as_str().cmp(right.as_str()),
	}
}

pub(in crate::ui) fn symbol_summary(
	store: &LocalWorkspaceFacade,
	symbol: &SymbolId,
) -> SymbolSummary {
	let Some(record) = symbol_by_id(store, symbol) else {
		return SymbolSummary::missing(symbol.clone());
	};
	SymbolSummary {
		id: record.id.clone(),
		lang: symbol_lang(store, record),
		kind: record.kind.clone(),
		name: record.name.clone(),
		file_path: symbol_source_path(store, record),
		compact_moniker: compact_identity(store, &record.identity),
		line_range: record.line_range,
		child_count: child_defs(store, &record.id).len(),
		change: change_badge_for_symbol(store, &record.id),
	}
}

pub(in crate::ui) fn symbol_detail(
	store: &LocalWorkspaceFacade,
	symbol: &SymbolId,
) -> SymbolDetail {
	SymbolDetail {
		symbol: symbol_summary(store, symbol),
		children: child_defs(store, symbol)
			.iter()
			.map(|child| symbol_summary(store, child))
			.collect(),
	}
}

pub(in crate::ui) fn symbol_references(
	store: &LocalWorkspaceFacade,
	symbol: &SymbolId,
) -> SymbolReferences {
	SymbolReferences {
		symbol: symbol_summary(store, symbol),
		incoming: reference_set(store, &incoming_refs_for_symbol(store, symbol), "source"),
		outgoing: reference_set(store, &outgoing_refs_for_symbol(store, symbol), "target"),
	}
}

pub(in crate::ui) fn source_snippet(
	store: &LocalWorkspaceFacade,
	symbol: &SymbolId,
	context: u32,
) -> Vec<SourceLine> {
	let Some(record) = symbol_by_id(store, symbol) else {
		return Vec::new();
	};
	let Some((start_line, end_line)) = record.line_range else {
		return Vec::new();
	};
	let Some(source) = source_file_by_id(store, &record.source) else {
		return Vec::new();
	};
	let first = start_line.saturating_sub(context).max(1);
	let last = end_line.saturating_add(context);
	source_lines(source, first, last)
		.into_iter()
		.map(|(number, text)| SourceLine {
			number,
			text,
			active: start_line <= number && number <= end_line,
		})
		.collect()
}

fn source_lines(source: &SourceFileRecord, first: u32, last: u32) -> Vec<(u32, String)> {
	if !source.text.is_empty() {
		return source
			.text
			.lines()
			.enumerate()
			.filter_map(|(idx, text)| {
				let number = idx as u32 + 1;
				(first <= number && number <= last).then(|| (number, text.to_string()))
			})
			.collect();
	}
	let Ok(file) = std::fs::File::open(&source.path) else {
		return Vec::new();
	};
	std::io::BufReader::new(file)
		.lines()
		.take(last as usize)
		.enumerate()
		.filter_map(|(idx, line)| {
			let number = idx as u32 + 1;
			(first <= number)
				.then(|| line.ok().map(|text| (number, text)))
				.flatten()
		})
		.collect()
}

pub(in crate::ui) fn search_symbols_filtered(
	store: &LocalWorkspaceFacade,
	query: &str,
	limit: usize,
	langs: &[Lang],
	kinds: &[String],
	shapes: &[Shape],
) -> Vec<SearchHit> {
	let Some(snapshot) = store.snapshot() else {
		return Vec::new();
	};
	let source_langs = source_lang_index(snapshot);
	let symbols_by_id = snapshot
		.index
		.symbols
		.iter()
		.map(|symbol| (symbol.id.clone(), symbol))
		.collect::<FxHashMap<_, _>>();
	let mut hits = store
		.view()
		.map(|view| view.search().search_symbols(query, limit.saturating_mul(4)))
		.unwrap_or_default()
		.into_iter()
		.filter(|hit| {
			symbols_by_id.get(&hit.symbol).is_some_and(|symbol| {
				symbol_matches_filters(symbol, &source_langs, langs, kinds, shapes)
			})
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

pub(in crate::ui) fn change_overview(store: &LocalWorkspaceFacade) -> ChangeOverview {
	let Some(snapshot) = store.snapshot() else {
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

pub(in crate::ui) fn change_rows(store: &LocalWorkspaceFacade) -> Vec<ChangeSummary> {
	store
		.view()
		.map(|view| {
			view.changes()
				.summaries()
				.into_iter()
				.map(|summary| change_summary_from_view(store, summary))
				.collect()
		})
		.unwrap_or_default()
}

pub(in crate::ui) fn change_summary(
	store: &LocalWorkspaceFacade,
	change: ChangeId,
) -> Option<ChangeSummary> {
	store
		.view()?
		.changes()
		.detail(&change)
		.map(|detail| change_summary_from_view(store, detail.summary))
}

pub(in crate::ui) fn change_detail(
	store: &LocalWorkspaceFacade,
	change: ChangeId,
) -> Option<ChangeDetail> {
	let detail = store.view()?.changes().detail(&change)?;
	Some(ChangeDetail {
		summary: change_summary_from_view(store, detail.summary),
		blast_radius: reference_set_from_view(store, detail.blast_radius),
	})
}

pub(in crate::ui) fn changed_defs(store: &LocalWorkspaceFacade) -> Vec<SymbolId> {
	store
		.snapshot()
		.map(|snapshot| snapshot.changes.changed_symbols.clone())
		.unwrap_or_default()
}

pub(in crate::ui) fn change_detail_for_symbol(
	store: &LocalWorkspaceFacade,
	symbol: &SymbolId,
) -> Option<ChangeDetail> {
	let change = store
		.snapshot()?
		.changes
		.changes
		.iter()
		.find(|change| change.symbol.as_ref() == Some(symbol))?
		.id
		.clone();
	change_detail(store, change)
}

pub(in crate::ui) fn change_count_for_file(store: &LocalWorkspaceFacade, file_idx: usize) -> usize {
	let Some(source) = source_file(store, file_idx) else {
		return 0;
	};
	store
		.snapshot()
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

pub(in crate::ui) fn usage_focus(
	store: &LocalWorkspaceFacade,
	symbol: SymbolId,
) -> Option<UsageFocus> {
	let record = symbol_by_id(store, &symbol)?;
	let target = moniker_for_identity(store, &record.identity)
		.unwrap_or_else(|| Moniker::from_canonical_bytes(record.identity.as_bytes().to_vec()));
	let refs = incoming_refs_for_symbol(store, &symbol);
	Some(UsageFocus {
		target,
		label: record.name.clone(),
		compact_moniker: compact_identity(store, &record.identity),
		refs: refs.clone(),
		contexts: usage_contexts(store, &refs),
		references: reference_set(store, &refs, "source"),
	})
}

pub(in crate::ui) fn usage_focus_for_target(
	store: &LocalWorkspaceFacade,
	target: Moniker,
	label: String,
) -> Option<UsageFocus> {
	let symbol = store
		.snapshot()?
		.index
		.symbols
		.iter()
		.find(|symbol| moniker_for_identity(store, &symbol.identity).as_ref() == Some(&target))
		.map(|symbol| symbol.id.clone())?;
	let mut focus = usage_focus(store, symbol)?;
	focus.target = target;
	focus.label = label;
	Some(focus)
}

pub(in crate::ui) fn unresolved_linkage_report(
	store: &LocalWorkspaceFacade,
	file_limit: usize,
	samples_per_file: usize,
) -> UnresolvedLinkageReport {
	let Some(snapshot) = store.snapshot() else {
		return UnresolvedLinkageReport::default();
	};
	let mut groups_by_file = FxHashMap::<SourceId, UnresolvedLinkageGroup>::default();
	for unresolved in &snapshot.linkage.unresolved {
		let Some(reference) = reference_by_id(store, &unresolved.reference) else {
			continue;
		};
		let Some(source) = source_file_by_id(store, &reference.source) else {
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
				target: compact_identity(store, &reference.target_identity),
				source: symbol_by_id(store, &reference.source_symbol)
					.map(|symbol| symbol.name.clone())
					.unwrap_or_else(|| reference.source_symbol.as_str().to_string()),
				location: reference_location(store, reference),
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

pub(in crate::ui) fn source_file(
	store: &LocalWorkspaceFacade,
	file_idx: usize,
) -> Option<&SourceFileRecord> {
	store.snapshot()?.index.sources.get(file_idx)
}

fn source_file_by_id<'a>(
	store: &'a LocalWorkspaceFacade,
	source: &SourceId,
) -> Option<&'a SourceFileRecord> {
	store
		.snapshot()?
		.index
		.sources
		.iter()
		.find(|candidate| &candidate.id == source)
}

fn symbol_by_id<'a>(store: &'a LocalWorkspaceFacade, id: &SymbolId) -> Option<&'a SymbolRecord> {
	store
		.snapshot()?
		.index
		.symbols
		.iter()
		.find(|symbol| &symbol.id == id)
}

fn reference_by_id<'a>(
	store: &'a LocalWorkspaceFacade,
	id: &ReferenceId,
) -> Option<&'a code_moniker_workspace::snapshot::ReferenceRecord> {
	store
		.snapshot()?
		.index
		.references
		.iter()
		.find(|reference| &reference.id == id)
}

fn symbol_lang(store: &LocalWorkspaceFacade, symbol: &SymbolRecord) -> Lang {
	source_file_by_id(store, &symbol.source)
		.and_then(|source| Lang::from_tag(&source.language))
		.unwrap_or(Lang::Rs)
}

fn symbol_source_path(store: &LocalWorkspaceFacade, symbol: &SymbolRecord) -> PathBuf {
	source_file_by_id(store, &symbol.source)
		.map(|source| PathBuf::from(&source.rel_path))
		.unwrap_or_default()
}

fn incoming_refs_for_symbol(store: &LocalWorkspaceFacade, symbol: &SymbolId) -> Vec<ReferenceId> {
	store
		.snapshot()
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

fn outgoing_refs_for_symbol(store: &LocalWorkspaceFacade, symbol: &SymbolId) -> Vec<ReferenceId> {
	store
		.snapshot()
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

fn reference_set(
	store: &LocalWorkspaceFacade,
	refs: &[ReferenceId],
	endpoint_label: &'static str,
) -> ReferenceSet {
	let mut groups = refs
		.iter()
		.filter_map(|id| reference_by_id(store, id))
		.map(|reference| reference_group(store, reference, endpoint_label))
		.collect::<Vec<_>>();
	groups.sort_by(reference_group_order);
	let files = groups
		.iter()
		.map(|group| group.location.clone())
		.collect::<BTreeSet<_>>()
		.len();
	let contexts = usage_contexts(store, refs).len();
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
	store: &LocalWorkspaceFacade,
	refs: code_moniker_workspace::snapshot::ReferenceSet,
) -> ReferenceSet {
	let ids = refs
		.groups
		.into_iter()
		.map(|reference| reference.reference)
		.collect::<Vec<_>>();
	reference_set(store, &ids, "source")
}

fn reference_group(
	store: &LocalWorkspaceFacade,
	reference: &code_moniker_workspace::snapshot::ReferenceRecord,
	endpoint_label: &'static str,
) -> ReferenceGroup {
	let source = symbol_by_id(store, &reference.source_symbol);
	ReferenceGroup {
		kinds: vec![reference.kind.clone()],
		actor: source
			.map(|symbol| symbol.name.clone())
			.unwrap_or_else(|| reference.source_symbol.as_str().to_string()),
		location: reference_location(store, reference),
		endpoint_label,
		endpoint: compact_identity(store, &reference.target_identity),
		confidence: reference
			.confidence
			.clone()
			.unwrap_or_else(|| "-".to_string()),
		receiver: reference.receiver.clone(),
		alias: reference.alias.clone(),
	}
}

fn reference_group_order(left: &ReferenceGroup, right: &ReferenceGroup) -> Ordering {
	reference_group_priority(left)
		.cmp(&reference_group_priority(right))
		.then_with(|| left.actor.cmp(&right.actor))
		.then_with(|| left.location.cmp(&right.location))
}

fn reference_group_priority(group: &ReferenceGroup) -> u8 {
	group
		.kinds
		.iter()
		.map(|kind| match kind.as_str() {
			"implements" | "extends" => 0,
			"method_call" | "calls" => 10,
			"instantiates" => 20,
			"reads" | "uses_type" | "returns_type" | "annotates" => 30,
			"imports_symbol" | "imports_module" => 40,
			_ => 50,
		})
		.min()
		.unwrap_or(50)
}

fn reference_location(
	store: &LocalWorkspaceFacade,
	reference: &code_moniker_workspace::snapshot::ReferenceRecord,
) -> String {
	let path = source_file_by_id(store, &reference.source)
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

fn usage_contexts(store: &LocalWorkspaceFacade, refs: &[ReferenceId]) -> Vec<SymbolId> {
	refs.iter()
		.filter_map(|id| reference_by_id(store, id))
		.map(|reference| reference.source_symbol.clone())
		.fold(Vec::new(), |mut out, symbol| {
			if !out.contains(&symbol) {
				out.push(symbol);
			}
			out
		})
}

fn change_badge_for_symbol(store: &LocalWorkspaceFacade, symbol: &SymbolId) -> Option<ChangeBadge> {
	let change = store
		.snapshot()?
		.changes
		.changes
		.iter()
		.find(|change| change.symbol.as_ref() == Some(symbol))?;
	Some(ChangeBadge {
		status: change.status,
		usage_count: incoming_refs_for_symbol(store, symbol).len(),
	})
}

fn change_summary_from_view(
	store: &LocalWorkspaceFacade,
	summary: code_moniker_workspace::snapshot::ChangeSummary,
) -> ChangeSummary {
	let source = summary
		.source
		.as_ref()
		.and_then(|id| source_file_by_id(store, id));
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
		compact_moniker: compact_identity(store, &summary.identity),
		line_range: summary.line_range,
		hunk_count: summary.hunk_count,
		usage_count: summary.usage_count,
	}
}

fn moniker_for_identity(store: &LocalWorkspaceFacade, identity: &str) -> Option<Moniker> {
	from_uri(
		identity,
		&UriConfig {
			scheme: store
				.snapshot()
				.map(|snapshot| snapshot.index.identity_scheme.as_str())
				.unwrap_or(crate::DEFAULT_SCHEME),
		},
	)
	.ok()
}

fn compact_identity(store: &LocalWorkspaceFacade, identity: &str) -> String {
	moniker_for_identity(store, identity)
		.as_ref()
		.map(compact_moniker)
		.unwrap_or_else(|| identity.to_string())
}

fn sort_defs_for_navigation(store: &LocalWorkspaceFacade, symbols: &mut [SymbolId]) {
	symbols.sort_by(|left, right| compare_defs_for_navigation(store, left, right));
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

fn symbol_matches_filters(
	symbol: &SymbolRecord,
	source_langs: &FxHashMap<SourceId, Lang>,
	langs: &[Lang],
	kinds: &[String],
	shapes: &[Shape],
) -> bool {
	let lang_matches = langs.is_empty()
		|| source_langs
			.get(&symbol.source)
			.is_some_and(|lang| langs.contains(lang));
	let has_kind_filter = !kinds.is_empty() || !shapes.is_empty();
	let kind_matches = !kinds.is_empty() && kinds.iter().any(|filter| filter == &symbol.kind);
	let shape_matches = !shapes.is_empty()
		&& shape_of(symbol.kind.as_bytes()).is_some_and(|shape| shapes.contains(&shape));
	lang_matches && (!has_kind_filter || kind_matches || shape_matches)
}

fn build_stats(snapshot: &WorkspaceSnapshot) -> SessionStats {
	fn millis(duration: std::time::Duration) -> u64 {
		duration.as_millis().try_into().unwrap_or(u64::MAX)
	}

	let source_langs = source_lang_index(snapshot);
	let mut stats = SessionStats {
		files: snapshot.index.sources.len(),
		defs: snapshot.index.symbols.len(),
		refs: snapshot.index.references.len(),
		scan_ms: millis(snapshot.timings.source_catalog),
		extract_ms: millis(snapshot.timings.extract_sources),
		index_ms: millis(snapshot.timings.semantic_index),
		linkage_ms: millis(snapshot.timings.linkage),
		changes_ms: millis(snapshot.timings.change_overlay),
		..SessionStats::default()
	};
	for lang in source_langs.values() {
		stats.by_lang.entry(lang.tag()).or_default().files += 1;
	}
	for symbol in &snapshot.index.symbols {
		if let Some(lang) = source_langs.get(&symbol.source) {
			stats.by_lang.entry(lang.tag()).or_default().defs += 1;
		}
		*stats.by_def_kind.entry(symbol.kind.clone()).or_default() += 1;
		if let Some(shape) = shape_of(symbol.kind.as_bytes()) {
			*stats.by_shape.entry(shape.as_str()).or_default() += 1;
		}
	}
	for reference in &snapshot.index.references {
		if let Some(lang) = source_langs.get(&reference.source) {
			stats.by_lang.entry(lang.tag()).or_default().refs += 1;
		}
		*stats.by_ref_kind.entry(reference.kind.clone()).or_default() += 1;
	}
	stats
}

fn source_lang_index(snapshot: &WorkspaceSnapshot) -> FxHashMap<SourceId, Lang> {
	snapshot
		.index
		.sources
		.iter()
		.filter_map(|source| Lang::from_tag(&source.language).map(|lang| (source.id.clone(), lang)))
		.collect()
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
