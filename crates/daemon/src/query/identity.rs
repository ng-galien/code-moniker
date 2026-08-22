use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use code_moniker_query::{
	AuditClusterDto, AuditSampleDto, AuditTotalsDto, AuditZoneDto, CountDto, GitRevisionDto,
	IdentityChildrenQuery, IdentityChildrenResult, IdentityGraphCoverage, IdentityGraphEdge,
	IdentityGraphPort, IdentityGraphQuery, IdentityGraphResult, IdentitySegmentDto,
	MetricsCouplingCoverage, MetricsCouplingQuery, MetricsCouplingResult,
	MetricsCouplingTargetUsage, Page, QueryCursor, QueryError, QueryResponse, QueryResult,
	ResolutionAuditResult, UnlinkedRefsDto, WorkspaceGeneration,
};
use code_moniker_workspace::glob::FilePathFilter;
use code_moniker_workspace::snapshot::{
	ExternalReferenceOrigin, ReferenceId, SymbolRecord, WorkspaceSnapshot, WorkspaceView,
};

use super::graph::{navigable_anchor, resolved_reference_target};
use crate::helpers::{DEFAULT_SCHEME, selected_roots, symbol_dto};
use crate::pagination::{page_rows, validate_page_cursor};
use crate::telemetry;

struct SegmentAgg<'a> {
	defs: usize,
	grandchildren: bool,
	direct: Option<&'a SymbolRecord>,
}

// One level of the identity tree: group every navigable definition under the
// prefix by its next identity segment. Segments that are definitions attach
// their SymbolDto; organizational segments only aggregate.
pub(crate) fn identity_children_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: IdentityChildrenQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let _ = selected_roots(roots, query.workspace.as_deref())?;
	let prefix = identity_path(query.prefix.trim_matches('/')).trim_matches('/');
	let mut children = identity_segments(snapshot, roots, prefix);
	if children.is_empty() {
		require_known_identity_prefix(snapshot, roots, prefix)?;
	}
	let total = children.len();
	children.truncate(query.limit);
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::IdentityChildren(IdentityChildrenResult {
			prefix: prefix.to_string(),
			total,
			children,
		}),
		next_cursor: None,
	})
}

fn require_known_identity_prefix(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	prefix: &str,
) -> Result<(), QueryError> {
	if prefix.is_empty() || identity_prefix_exists(snapshot, prefix) {
		return Ok(());
	}
	let heads = identity_segments(snapshot, roots, "")
		.into_iter()
		.map(|segment| segment.segment)
		.take(8)
		.collect::<Vec<_>>();
	let guidance = if heads.is_empty() {
		"the workspace has no indexed symbols".to_string()
	} else {
		format!(
			"a prefix is a head sequence of canonical identity segments; valid heads: {}",
			heads.join(", ")
		)
	};
	Err(QueryError::new(
		"prefix_not_found",
		format!("no symbol identity starts with `{prefix}`; {guidance}"),
	))
}

pub(super) fn directory_or_unknown_focus_error(
	snapshot: &WorkspaceSnapshot,
	focus: &str,
) -> QueryError {
	let dir_prefix = format!("{}/", focus.trim_end_matches('/'));
	let is_directory = snapshot
		.index
		.sources
		.iter()
		.any(|source| source.rel_path.starts_with(&dir_prefix));
	if is_directory {
		return QueryError::new(
			"focus_is_directory",
			format!(
				"focus `{focus}` is a directory; the unit graph takes a symbol URI or a \
				 file path - for scope-level coupling use identity.graph (list valid \
				 heads with identity.children prefix:\"\"), or pick a file via \
				 symbol.search path:\"{focus}/**\""
			),
		);
	}
	QueryError::new(
		"focus_not_found",
		format!("no symbol or file matches focus `{focus}`"),
	)
}

fn identity_prefix_exists(snapshot: &WorkspaceSnapshot, prefix: &str) -> bool {
	snapshot
		.index
		.symbols
		.iter()
		.filter(|symbol| symbol.navigable)
		.any(|symbol| identity_in_scope(identity_path(symbol.identity.as_ref()), prefix))
}

fn identity_segments(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	prefix: &str,
) -> Vec<IdentitySegmentDto> {
	identity_segments_scoped(snapshot, roots, prefix, &FilePathFilter::default())
}

fn identity_segments_scoped(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	prefix: &str,
	path_filter: &FilePathFilter,
) -> Vec<IdentitySegmentDto> {
	let sources_view = WorkspaceView::new(snapshot).sources();
	let mut groups: BTreeMap<&str, SegmentAgg> = BTreeMap::new();
	for symbol in snapshot.index.symbols.iter() {
		if !symbol.navigable {
			continue;
		}
		let Some(source) = sources_view.record(&symbol.source) else {
			continue;
		};
		if !path_filter.matches(&source.rel_path) {
			continue;
		}
		let Some(rest) = identity_rest(identity_path(symbol.identity.as_ref()), prefix) else {
			continue;
		};
		let (segment, tail) = match rest.split_once('/') {
			Some((segment, tail)) => (segment, Some(tail)),
			None => (rest, None),
		};
		if segment.is_empty() {
			continue;
		}
		let entry = groups.entry(segment).or_insert(SegmentAgg {
			defs: 0,
			grandchildren: false,
			direct: None,
		});
		match tail {
			None => entry.direct = Some(symbol),
			Some(_) => {
				entry.defs += 1;
				entry.grandchildren = true;
			}
		}
	}
	groups
		.into_iter()
		.map(|(segment, agg)| identity_segment_dto(segment, agg, prefix, &sources_view, roots))
		.collect()
}

// The scoped exploration graph: the prefix's children as nodes, every
// resolved reference rolled up to the pair of child segments it connects,
// and boundary crossings aggregated into ports at the scope's own depth.
pub(crate) fn identity_graph_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: IdentityGraphQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let _ = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let prefix = identity_path(query.prefix.trim_matches('/'))
		.trim_matches('/')
		.to_string();
	let nodes = identity_segments_scoped(snapshot, roots, &prefix, &path_filter);
	if nodes.is_empty() {
		require_known_identity_prefix(snapshot, roots, &prefix)?;
	}
	let symbols_view = WorkspaceView::new(snapshot).symbols();
	let sources_view = WorkspaceView::new(snapshot).sources();
	let mut edges: BTreeMap<(String, String), (BTreeSet<String>, usize)> = BTreeMap::new();
	let mut ports_in: BTreeMap<String, (BTreeSet<String>, usize)> = BTreeMap::new();
	let mut ports_out: BTreeMap<String, (BTreeSet<String>, usize)> = BTreeMap::new();
	let classifier = UnlinkedClassifier::new(snapshot);
	let mut unlinked = UnlinkedRefsDto::default();
	let port_depth = if prefix.is_empty() {
		1
	} else {
		prefix.split('/').count() + 1
	};
	for reference in snapshot.index.references.iter() {
		let Some(source) = navigable_anchor(&symbols_view, reference.source_symbol) else {
			continue;
		};
		let source_selected = sources_view
			.record(&source.source)
			.is_some_and(|record| path_filter.matches(&record.rel_path));
		let source_segment = source_selected
			.then(|| scope_segment(source, &prefix))
			.flatten();
		let Some(target_id) = resolved_reference_target(snapshot, &reference.id) else {
			if source_segment.is_some() {
				classifier.tally(&reference.id, &mut unlinked);
			}
			continue;
		};
		let Some(target) = navigable_anchor(&symbols_view, target_id) else {
			continue;
		};
		let target_selected = sources_view
			.record(&target.source)
			.is_some_and(|record| path_filter.matches(&record.rel_path));
		let kind = reference.kind.as_str();
		let target_segment = target_selected
			.then(|| scope_segment(target, &prefix))
			.flatten();
		match (source_segment, target_segment) {
			(Some(from), Some(to)) => {
				if from != to {
					let entry = edges.entry((from, to)).or_default();
					entry.0.insert(kind.to_string());
					entry.1 += 1;
				}
			}
			(Some(_), None) => bump_port(
				&mut ports_out,
				truncate_identity(identity_path(target.identity.as_ref()), port_depth),
				kind,
			),
			(None, Some(_)) => bump_port(
				&mut ports_in,
				truncate_identity(identity_path(source.identity.as_ref()), port_depth),
				kind,
			),
			(None, None) => {}
		}
	}
	let edges = edges
		.into_iter()
		.map(|((source, target), (kinds, count))| IdentityGraphEdge {
			source,
			target,
			kinds: kinds.into_iter().collect(),
			count,
		})
		.collect::<Vec<_>>();
	let ports_in = into_ports(ports_in);
	let ports_out = into_ports(ports_out);
	let graph_page = page_identity_graph(
		IdentityGraphSections {
			nodes,
			edges,
			ports_in,
			ports_out,
		},
		query.min_count,
		page,
		current_generation,
	)?;
	let result = IdentityGraphResult {
		prefix,
		path: query.path,
		min_count: query.min_count,
		coverage: graph_page.coverage,
		nodes: graph_page.nodes,
		edges: graph_page.edges,
		ports_in: graph_page.ports_in,
		ports_out: graph_page.ports_out,
		unlinked,
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::IdentityGraph(Box::new(result)),
		next_cursor: graph_page.next_cursor,
	})
}

pub(crate) fn metrics_coupling_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: MetricsCouplingQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let git = coupling_git_revision(&selected_roots);
	let snapshot_label = query
		.snapshot
		.as_deref()
		.map(str::trim)
		.filter(|label| !label.is_empty())
		.unwrap_or("current")
		.to_string();
	let from = identity_path(query.from.trim_matches('/'))
		.trim_matches('/')
		.to_string();
	let to = identity_path(query.to.trim_matches('/'))
		.trim_matches('/')
		.to_string();
	require_known_identity_prefix(snapshot, roots, &from)?;
	require_known_identity_prefix(snapshot, roots, &to)?;

	let relations = query
		.relation
		.iter()
		.map(String::as_str)
		.collect::<HashSet<_>>();
	let symbols = WorkspaceView::new(snapshot).symbols();
	let source_files = snapshot
		.index
		.symbols
		.iter()
		.filter(|symbol| {
			symbol.navigable && identity_in_scope(identity_path(symbol.identity.as_ref()), &from)
		})
		.map(|symbol| symbol.id.file())
		.collect::<BTreeSet<_>>();
	let classifier = UnlinkedClassifier::new(snapshot);
	let mut source_references = 0usize;
	let mut resolved_source_references = 0usize;
	let mut references = 0usize;
	let mut same_symbol_references = 0usize;
	let mut source_symbols = BTreeSet::new();
	let mut target_symbols = BTreeSet::new();
	let mut connections = BTreeSet::new();
	let mut by_kind = BTreeMap::<String, usize>::new();
	let mut by_target = BTreeMap::<String, usize>::new();
	let measure_boundary_targets = from != to;
	let mut unlinked = UnlinkedRefsDto::default();

	for reference in source_files
		.iter()
		.flat_map(|file| snapshot.index.references.file_records(*file))
	{
		let Some(source) = navigable_anchor(&symbols, reference.source_symbol) else {
			continue;
		};
		if !identity_in_scope(identity_path(source.identity.as_ref()), &from) {
			continue;
		}
		let kind = reference.kind.as_str();
		if !relations.is_empty() && !relations.contains(kind) {
			continue;
		}
		source_references += 1;
		let Some(target_id) = resolved_reference_target(snapshot, &reference.id) else {
			classifier.tally(&reference.id, &mut unlinked);
			continue;
		};
		resolved_source_references += 1;
		let Some(target) = navigable_anchor(&symbols, target_id) else {
			continue;
		};
		if !identity_in_scope(identity_path(target.identity.as_ref()), &to) {
			continue;
		}
		if source.id == target.id {
			same_symbol_references += 1;
			continue;
		}
		references += 1;
		source_symbols.insert(source.id);
		target_symbols.insert(target.id);
		connections.insert((source.id, target.id));
		*by_kind.entry(kind.to_string()).or_default() += 1;
		if measure_boundary_targets {
			*by_target
				.entry(identity_path(target.identity.as_ref()).to_string())
				.or_default() += 1;
		}
	}

	let mut result = MetricsCouplingResult {
		from,
		to,
		relation: query.relation,
		snapshot: snapshot_label,
		git,
		export_requested: query.export,
		export_recorded: false,
		references,
		connections: connections.len(),
		source_symbols: source_symbols.len(),
		target_symbols: target_symbols.len(),
		same_symbol_references,
		coverage: MetricsCouplingCoverage {
			source_references,
			resolved_source_references,
		},
		by_kind: by_kind
			.into_iter()
			.map(|(name, count)| CountDto { name, count })
			.collect(),
		by_target: by_target
			.into_iter()
			.map(|(moniker, references)| MetricsCouplingTargetUsage {
				moniker,
				references,
			})
			.collect(),
		unlinked,
	};
	if result.export_requested {
		result.export_recorded = telemetry::record_coupling_metrics(&result);
	}
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::MetricsCoupling(Box::new(result)),
		next_cursor: None,
	})
}

fn coupling_git_revision(roots: &[&PathBuf]) -> Option<GitRevisionDto> {
	let revisions = roots
		.iter()
		.map(|root| code_moniker_workspace::changes::diff::git_revision(root))
		.collect::<Result<Vec<_>, _>>()
		.ok()?;
	let first = revisions.first()?;
	if revisions.iter().any(|revision| revision != first) {
		return None;
	}
	Some(GitRevisionDto {
		branch: first.branch.clone(),
		commit: first.commit.clone(),
		dirty: first.dirty,
	})
}

struct IdentityGraphPage {
	nodes: Vec<IdentitySegmentDto>,
	edges: Vec<IdentityGraphEdge>,
	ports_in: Vec<IdentityGraphPort>,
	ports_out: Vec<IdentityGraphPort>,
	coverage: IdentityGraphCoverage,
	next_cursor: Option<QueryCursor>,
}

struct IdentityGraphSections {
	nodes: Vec<IdentitySegmentDto>,
	edges: Vec<IdentityGraphEdge>,
	ports_in: Vec<IdentityGraphPort>,
	ports_out: Vec<IdentityGraphPort>,
}

fn page_identity_graph(
	sections: IdentityGraphSections,
	min_count: usize,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<IdentityGraphPage, QueryError> {
	let IdentityGraphSections {
		nodes,
		edges,
		ports_in,
		ports_out,
	} = sections;
	let nodes_total = nodes.len();
	let edges_total = edges.len();
	let ports_in_total = ports_in.len();
	let ports_out_total = ports_out.len();
	let edges = edges
		.into_iter()
		.filter(|edge| edge.count >= min_count)
		.collect::<Vec<_>>();
	let ports_in = ports_in
		.into_iter()
		.filter(|port| port.count >= min_count)
		.collect::<Vec<_>>();
	let ports_out = ports_out
		.into_iter()
		.filter(|port| port.count >= min_count)
		.collect::<Vec<_>>();
	let edges_matching = edges.len();
	let ports_in_matching = ports_in.len();
	let ports_out_matching = ports_out.len();
	let rows_total = nodes_total + edges_total + ports_in_total + ports_out_total;
	let rows_matching = nodes_total + edges_matching + ports_in_matching + ports_out_matching;
	let rows = nodes
		.into_iter()
		.map(IdentityGraphRow::Node)
		.chain(edges.into_iter().map(IdentityGraphRow::Edge))
		.chain(ports_in.into_iter().map(IdentityGraphRow::PortIn))
		.chain(ports_out.into_iter().map(IdentityGraphRow::PortOut))
		.collect();
	let paged = page_rows(rows, page, current_generation)?;
	let mut nodes = Vec::new();
	let mut edges = Vec::new();
	let mut ports_in = Vec::new();
	let mut ports_out = Vec::new();
	for row in paged.items {
		match row {
			IdentityGraphRow::Node(row) => nodes.push(row),
			IdentityGraphRow::Edge(row) => edges.push(row),
			IdentityGraphRow::PortIn(row) => ports_in.push(row),
			IdentityGraphRow::PortOut(row) => ports_out.push(row),
		}
	}
	Ok(IdentityGraphPage {
		coverage: IdentityGraphCoverage {
			rows_total,
			rows_matching,
			rows_emitted: nodes.len() + edges.len() + ports_in.len() + ports_out.len(),
			nodes_total,
			nodes_emitted: nodes.len(),
			edges_total,
			edges_matching,
			edges_emitted: edges.len(),
			ports_in_total,
			ports_in_matching,
			ports_in_emitted: ports_in.len(),
			ports_out_total,
			ports_out_matching,
			ports_out_emitted: ports_out.len(),
		},
		nodes,
		edges,
		ports_in,
		ports_out,
		next_cursor: paged.next_cursor,
	})
}

enum IdentityGraphRow {
	Node(IdentitySegmentDto),
	Edge(IdentityGraphEdge),
	PortIn(IdentityGraphPort),
	PortOut(IdentityGraphPort),
}

pub(crate) fn resolution_audit_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: code_moniker_query::ResolutionAuditQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let _ = selected_roots(roots, query.workspace.as_deref())?;
	validate_page_cursor(&page, current_generation)?;
	let prefix = identity_path(query.prefix.trim_matches('/'))
		.trim_matches('/')
		.to_string();
	let sample_offset = page
		.cursor
		.as_ref()
		.map(|cursor| cursor.offset)
		.unwrap_or(0);
	let drill_down = query.cluster.is_some();
	let options = code_moniker_workspace::audit::AuditOptions {
		cluster_limit: query.limit.clamp(1, 200),
		sample_limit: if drill_down {
			query.limit.clamp(1, 200)
		} else {
			code_moniker_workspace::audit::AuditOptions::default().sample_limit
		},
		sample_offset,
		cluster: query.cluster,
		..code_moniker_workspace::audit::AuditOptions::default()
	};
	let audit = code_moniker_workspace::audit::resolution_audit(snapshot, &prefix, options);
	let next_cursor = drill_down
		.then(|| audit.clusters.first())
		.flatten()
		.filter(|cluster| sample_offset + cluster.samples.len() < cluster.count)
		.map(|cluster| QueryCursor {
			offset: sample_offset + cluster.samples.len(),
			generation: current_generation,
		});
	let result = ResolutionAuditResult {
		prefix,
		totals: AuditTotalsDto {
			references: audit.totals.references,
			resolved: audit.totals.resolved,
			unique: audit.totals.unique,
			candidate: audit.totals.candidate,
			external: audit.totals.external,
			sdk: audit.totals.sdk,
			dependency: audit.totals.dependency,
			injected_external: audit.totals.injected_external,
			unknown_external: audit.totals.unknown_external,
			dynamic: audit.totals.dynamic,
			blocked: audit.totals.blocked,
			unresolved: audit.totals.unresolved,
			explained: audit.totals.explained,
			weak_or_unexplained: audit.totals.weak_or_unexplained,
			name_match_resolved: audit.totals.name_match_resolved,
			name_match_candidate: audit.totals.name_match_candidate,
		},
		clusters: audit
			.clusters
			.iter()
			.map(|cluster| AuditClusterDto {
				id: cluster.id.clone(),
				pattern: code_moniker_workspace::audit::pattern_label(&cluster.pattern),
				count: cluster.count,
				samples: cluster
					.samples
					.iter()
					.map(|sample| AuditSampleDto {
						file: sample.file.clone(),
						line_range: sample.line_range,
						snippet: audit_sample_snippet(snapshot, sample),
						source: sample.source.clone(),
						call_name: sample.call_name.clone(),
						receiver: sample.receiver.clone(),
						target: sample.target.clone(),
						evidence: sample.evidence.clone(),
						constraints: sample.constraints.clone(),
						candidates: sample.candidates.clone(),
					})
					.collect(),
			})
			.collect(),
		zones: audit
			.zones
			.iter()
			.map(|zone| AuditZoneDto {
				zone: zone.zone.clone(),
				unresolved: zone.unresolved,
				dominant_pattern: zone.dominant_pattern.clone(),
			})
			.collect(),
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::ResolutionAudit(Box::new(result)),
		next_cursor,
	})
}

fn audit_sample_snippet(
	snapshot: &WorkspaceSnapshot,
	sample: &code_moniker_workspace::audit::AuditSample,
) -> String {
	if !sample.snippet.is_empty() {
		return sample.snippet.clone();
	}
	let Some(line_range) = sample.line_range else {
		return String::new();
	};
	let Some(source) = snapshot
		.index
		.sources
		.iter()
		.find(|source| source.rel_path == sample.file)
	else {
		return String::new();
	};
	let Ok(text) = std::fs::read_to_string(&source.path) else {
		return String::new();
	};
	bounded_source_excerpt(&text, line_range)
}

pub(crate) fn bounded_source_excerpt(source: &str, (start, end): (u32, u32)) -> String {
	let line_count = end.saturating_sub(start).saturating_add(1).min(3) as usize;
	let excerpt = source
		.lines()
		.skip(start.saturating_sub(1) as usize)
		.take(line_count)
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.collect::<Vec<_>>()
		.join(" ");
	excerpt.chars().take(240).collect()
}

// Classifies references without an in-workspace target so external-by-design
// links never masquerade as resolution gaps in graph outputs.
pub(super) struct UnlinkedClassifier {
	external: HashMap<ReferenceId, ExternalReferenceOrigin>,
	candidate: HashSet<ReferenceId>,
	dynamic: HashSet<ReferenceId>,
	manifest_blocked: HashSet<ReferenceId>,
	unresolved: HashMap<ReferenceId, code_moniker_workspace::snapshot::UnresolvedReason>,
}

impl UnlinkedClassifier {
	pub(super) fn new(snapshot: &WorkspaceSnapshot) -> Self {
		Self {
			external: snapshot
				.linkage
				.external
				.iter()
				.map(|reference| (reference.reference, reference.origin))
				.collect(),
			candidate: snapshot
				.linkage
				.candidates
				.iter()
				.map(|reference| reference.reference)
				.collect(),
			dynamic: snapshot
				.linkage
				.dynamic
				.iter()
				.map(|reference| reference.reference)
				.collect(),
			manifest_blocked: snapshot
				.linkage
				.blocked
				.iter()
				.chain(snapshot.linkage.manifest_blocked.iter())
				.map(|reference| reference.reference)
				.collect(),
			unresolved: snapshot
				.linkage
				.unresolved
				.iter()
				.map(|reference| (reference.reference, reference.reason))
				.collect(),
		}
	}

	pub(super) fn tally(&self, reference: &ReferenceId, unlinked: &mut UnlinkedRefsDto) {
		if let Some(origin) = self.external.get(reference) {
			unlinked.external += 1;
			match origin {
				ExternalReferenceOrigin::Sdk => unlinked.sdk += 1,
				ExternalReferenceOrigin::Dependency => unlinked.dependency += 1,
				ExternalReferenceOrigin::Injected => unlinked.injected_external += 1,
				ExternalReferenceOrigin::UnknownExternal => unlinked.unknown_external += 1,
			}
		} else if self.candidate.contains(reference) {
			unlinked.candidate += 1;
		} else if self.dynamic.contains(reference) {
			unlinked.dynamic += 1;
		} else if self.manifest_blocked.contains(reference) {
			unlinked.manifest_blocked += 1;
		} else {
			unlinked.unresolved += 1;
			let reason = self
				.unresolved
				.get(reference)
				.map_or("unclassified", |reason| reason.as_str());
			*unlinked
				.unresolved_reasons
				.entry(reason.to_string())
				.or_default() += 1;
		}
	}
}

// The child identity of the scope that contains this symbol, if any.
fn scope_segment(symbol: &SymbolRecord, prefix: &str) -> Option<String> {
	let rest = identity_rest(identity_path(symbol.identity.as_ref()), prefix)?;
	let segment = rest
		.split('/')
		.next()
		.filter(|segment| !segment.is_empty())?;
	Some(if prefix.is_empty() {
		segment.to_string()
	} else {
		format!("{prefix}/{segment}")
	})
}

fn truncate_identity(path: &str, segments: usize) -> String {
	path.split('/').take(segments).collect::<Vec<_>>().join("/")
}

fn bump_port(map: &mut BTreeMap<String, (BTreeSet<String>, usize)>, key: String, kind: &str) {
	let entry = map.entry(key).or_default();
	entry.0.insert(kind.to_string());
	entry.1 += 1;
}

fn into_ports(map: BTreeMap<String, (BTreeSet<String>, usize)>) -> Vec<IdentityGraphPort> {
	map.into_iter()
		.map(|(identity, (kinds, count))| IdentityGraphPort {
			identity,
			kinds: kinds.into_iter().collect(),
			count,
		})
		.collect()
}

fn identity_segment_dto(
	segment: &str,
	agg: SegmentAgg<'_>,
	prefix: &str,
	sources_view: &code_moniker_workspace::snapshot::SourceView<'_>,
	roots: &[PathBuf],
) -> IdentitySegmentDto {
	let (kind, name) = segment.split_once(':').unwrap_or(("", segment));
	let identity = if prefix.is_empty() {
		segment.to_string()
	} else {
		format!("{prefix}/{segment}")
	};
	let symbol = agg.direct.and_then(|record| {
		let source = sources_view.record(&record.source)?;
		Some(Box::new(symbol_dto(record, source, roots)))
	});
	IdentitySegmentDto {
		segment: segment.to_string(),
		kind: kind.to_string(),
		name: name.to_string(),
		identity,
		defs: agg.defs,
		has_children: agg.grandchildren,
		symbol,
	}
}

// Record identities are full moniker URIs; the identity tree navigates the
// path AFTER the scheme and root anchor (`code+moniker://./`). Full URIs are
// accepted as prefixes and normalized to the same space.
fn identity_path(identity: &str) -> &str {
	let Some(rest) = identity.strip_prefix(DEFAULT_SCHEME) else {
		return identity;
	};
	match rest.split_once('/') {
		Some((_, path)) => path,
		None => "",
	}
}

pub(crate) fn identity_rest<'a>(identity: &'a str, prefix: &str) -> Option<&'a str> {
	if prefix.is_empty() {
		return Some(identity);
	}
	if identity.len() > prefix.len()
		&& identity.starts_with(prefix)
		&& identity.as_bytes()[prefix.len()] == b'/'
	{
		Some(&identity[prefix.len() + 1..])
	} else {
		None
	}
}

fn identity_in_scope(identity: &str, prefix: &str) -> bool {
	prefix.is_empty() || identity == prefix || identity_rest(identity, prefix).is_some()
}
