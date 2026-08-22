use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use code_moniker_core::core::shape::Shape;
use code_moniker_query::{
	GraphCorridorEdge, GraphCorridorQuery, GraphCorridorResult, GraphCorridorSearchStats,
	GraphPathCoverage, GraphPathExpectation, GraphPathQuery, GraphPathResult, GraphPathSearchStats,
	GraphPathStep, GraphPathVerdict, GraphSectionCoverage, GraphSymbolScope, QueryError,
	QueryResponse, QueryResult, SymbolDto, SymbolGraphCoverage, SymbolGraphEdge, SymbolGraphFocus,
	SymbolGraphNeighbor, SymbolGraphQuery, SymbolGraphResult, UnlinkedRefsDto, UsageDirection,
	WorkspaceGeneration,
};
use code_moniker_workspace::glob::FilePathFilter;
use code_moniker_workspace::snapshot::{
	BoundedCorridorScope, BoundedCorridorSetRequest, BoundedPathCoverage, BoundedPathLimits,
	BoundedPathScope, BoundedPathSetRequest, ReferenceId, SourceId, SymbolId, SymbolRecord,
	SymbolSet, WorkspaceSnapshot, WorkspaceView,
};

use super::identity::{UnlinkedClassifier, directory_or_unknown_focus_error};
use crate::helpers::{
	find_symbol, selected_roots, source_root, symbol_dto, symbol_scope_for_roots,
};

pub(super) enum UnitBoundary {
	IdentityPrefix { prefix: String, slot: usize },
	File { source: SourceId, slot: usize },
}

impl UnitBoundary {
	fn slot(&self) -> usize {
		match self {
			Self::IdentityPrefix { slot, .. } | Self::File { slot, .. } => *slot,
		}
	}

	fn contains(&self, symbol: &SymbolRecord) -> bool {
		match self {
			Self::IdentityPrefix { prefix, .. } => {
				let identity = symbol.identity.as_ref();
				identity == prefix
					|| (identity.len() > prefix.len()
						&& identity.starts_with(prefix.as_str())
						&& identity.as_bytes()[prefix.len()] == b'/')
			}
			Self::File { source, .. } => &symbol.source == source,
		}
	}
}

struct NeighborBag {
	entries: BTreeMap<SymbolId, (BTreeSet<String>, usize)>,
}

impl NeighborBag {
	fn new() -> Self {
		Self {
			entries: BTreeMap::new(),
		}
	}

	fn add(&mut self, symbol: SymbolId, kind: &str) {
		let entry = self.entries.entry(symbol).or_default();
		entry.0.insert(kind.to_string());
		entry.1 += 1;
	}

	fn into_neighbors(
		self,
		snapshot: &WorkspaceSnapshot,
		roots: &[PathBuf],
	) -> Vec<SymbolGraphNeighbor> {
		let symbols = WorkspaceView::new(snapshot).symbols();
		let sources = WorkspaceView::new(snapshot).sources();
		let mut neighbors: Vec<SymbolGraphNeighbor> = self
			.entries
			.into_iter()
			.filter_map(|(id, (kinds, count))| {
				let symbol = symbols.find(&id)?;
				let source = sources.record(&symbol.source)?;
				Some(SymbolGraphNeighbor {
					symbol: symbol_dto(symbol, source, roots),
					kinds: kinds.into_iter().collect(),
					count,
				})
			})
			.collect();
		neighbors.sort_by(|a, b| {
			(&a.symbol.file, a.symbol.line_range).cmp(&(&b.symbol.file, b.symbol.line_range))
		});
		neighbors
	}
}

pub(crate) fn symbol_graph_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolGraphQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let symbol_scope = symbol_scope_for_roots(snapshot, roots, &selected_roots);
	let (boundary, focus) = resolve_unit_boundary(
		snapshot,
		roots,
		&selected_roots,
		&symbol_scope,
		&query.focus,
	)?;
	let symbols_view = WorkspaceView::new(snapshot).symbols();
	let sources_view = WorkspaceView::new(snapshot).sources();
	let slot = boundary.slot();
	let members: Vec<&SymbolRecord> = snapshot
		.index
		.symbols
		.file_records(slot)
		.iter()
		.filter(|symbol| symbol.navigable && boundary.contains(symbol))
		.collect();
	let mut internal: BTreeMap<(SymbolId, SymbolId), (BTreeSet<String>, usize)> = BTreeMap::new();
	let mut callees = NeighborBag::new();
	let mut callers = NeighborBag::new();
	let classifier = UnlinkedClassifier::new(snapshot);
	let mut unlinked = UnlinkedRefsDto::default();
	for reference in snapshot.index.references.file_records(slot).iter() {
		let Some(source) = navigable_anchor(&symbols_view, reference.source_symbol) else {
			continue;
		};
		if !boundary.contains(source) {
			continue;
		}
		let Some(target_id) = resolved_reference_target(snapshot, &reference.id) else {
			classifier.tally(&reference.id, &mut unlinked);
			continue;
		};
		let Some(target) = navigable_anchor(&symbols_view, target_id) else {
			continue;
		};
		let kind = reference.kind.as_str();
		if boundary.contains(target) {
			let entry = internal.entry((source.id, target.id)).or_default();
			entry.0.insert(kind.to_string());
			entry.1 += 1;
		} else {
			callees.add(target.id, kind);
		}
	}
	for member in &members {
		for reference_id in incoming_reference_ids(snapshot, &member.id) {
			let Some(reference) = WorkspaceView::new(snapshot)
				.references()
				.reference(&reference_id)
			else {
				continue;
			};
			let Some(source) = navigable_anchor(&symbols_view, reference.source_symbol) else {
				continue;
			};
			if boundary.contains(source) {
				continue;
			}
			callers.add(source.id, reference.kind.as_str());
		}
	}
	let mut member_dtos: Vec<SymbolDto> = members
		.iter()
		.filter_map(|symbol| {
			let source = sources_view.record(&symbol.source)?;
			Some(symbol_dto(symbol, source, roots))
		})
		.collect();
	member_dtos.sort_by_key(|dto| dto.line_range);
	let graph = filter_symbol_graph_sections(
		internal,
		callers.into_neighbors(snapshot, roots),
		callees.into_neighbors(snapshot, roots),
		&query,
		member_dtos.len(),
	);
	let member_total = member_dtos.len();
	member_dtos.truncate(query.limit);
	let result = SymbolGraphResult {
		focus,
		direction: query.direction,
		coverage: SymbolGraphCoverage {
			members: GraphSectionCoverage {
				total: member_total,
				matching: member_total,
				returned: member_dtos.len(),
			},
			..graph.coverage
		},
		members: member_dtos,
		internal_edges: graph.internal_edges,
		callers: graph.callers,
		callees: graph.callees,
		unlinked,
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolGraph(Box::new(result)),
		next_cursor: None,
	})
}

struct FilteredSymbolGraph {
	internal_edges: Vec<SymbolGraphEdge>,
	callers: Vec<SymbolGraphNeighbor>,
	callees: Vec<SymbolGraphNeighbor>,
	coverage: SymbolGraphCoverage,
}

fn filter_symbol_graph_sections(
	internal: BTreeMap<(SymbolId, SymbolId), (BTreeSet<String>, usize)>,
	callers: Vec<SymbolGraphNeighbor>,
	callees: Vec<SymbolGraphNeighbor>,
	query: &SymbolGraphQuery,
	member_count: usize,
) -> FilteredSymbolGraph {
	let relation_matches = |kinds: &[String]| {
		query.relation.is_empty()
			|| kinds
				.iter()
				.any(|kind| query.relation.iter().any(|expected| expected == kind))
	};
	let internal_edges = internal
		.into_iter()
		.map(|((source, target), (kinds, count))| SymbolGraphEdge {
			source: source.to_string(),
			target: target.to_string(),
			kinds: kinds.into_iter().collect(),
			count,
		})
		.collect::<Vec<_>>();
	let internal_edges_total = internal_edges.len();
	let mut internal_edges = internal_edges
		.into_iter()
		.filter(|edge| edge.count >= query.min_count && relation_matches(&edge.kinds))
		.collect::<Vec<_>>();
	let internal_edges_matching = internal_edges.len();
	if !query.include_internal {
		internal_edges.clear();
	}
	let filter_neighbors = |neighbors: Vec<SymbolGraphNeighbor>| {
		neighbors
			.into_iter()
			.filter(|neighbor| {
				neighbor.count >= query.min_count && relation_matches(&neighbor.kinds)
			})
			.collect::<Vec<_>>()
	};
	let callers_total = callers.len();
	let callees_total = callees.len();
	let mut callers = filter_neighbors(callers);
	let mut callees = filter_neighbors(callees);
	let callers_matching = callers.len();
	let callees_matching = callees.len();
	match query.direction {
		UsageDirection::Incoming => callees.clear(),
		UsageDirection::Outgoing => callers.clear(),
		UsageDirection::Both => {}
	}
	internal_edges.truncate(query.limit);
	callers.truncate(query.limit);
	callees.truncate(query.limit);
	FilteredSymbolGraph {
		coverage: SymbolGraphCoverage {
			members: GraphSectionCoverage {
				total: member_count,
				matching: member_count,
				returned: member_count.min(query.limit),
			},
			internal_edges: GraphSectionCoverage {
				total: internal_edges_total,
				matching: internal_edges_matching,
				returned: internal_edges.len(),
			},
			callers: GraphSectionCoverage {
				total: callers_total,
				matching: callers_matching,
				returned: callers.len(),
			},
			callees: GraphSectionCoverage {
				total: callees_total,
				matching: callees_matching,
				returned: callees.len(),
			},
		},
		internal_edges,
		callers,
		callees,
	}
}

pub(crate) fn graph_path_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: GraphPathQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let symbol_scope = symbol_scope_for_roots(snapshot, roots, &selected_roots);
	let from = find_symbol(snapshot, &symbol_scope, &query.from)?;
	let to = find_symbol(snapshot, &symbol_scope, &query.to)?;
	let view = WorkspaceView::new(snapshot);
	let sources = view.sources();
	let from_source = sources
		.record(&from.source)
		.ok_or_else(|| QueryError::new("source_not_found", "source symbol source not found"))?;
	let to_source = sources
		.record(&to.source)
		.ok_or_else(|| QueryError::new("source_not_found", "target symbol source not found"))?;
	for (uri, source) in [(&query.from, from_source), (&query.to, to_source)] {
		if source_root(roots, &selected_roots, source).is_none() {
			return Err(QueryError::new(
				"symbol_not_in_workspace",
				format!("symbol {uri} is not in the selected workspace"),
			));
		}
	}
	let path_scope = if selected_roots.len() == roots.len() {
		BoundedPathScope::all()
	} else {
		BoundedPathScope::from_source_roots(roots.iter().enumerate().filter_map(|(index, root)| {
			selected_roots
				.iter()
				.any(|selected| selected.as_path() == root.as_path())
				.then_some(index)
		}))
	};
	let (from_endpoints, to_endpoints) =
		natural_graph_endpoint_sets(snapshot, from, to, query.max_symbols, "path")?;
	let search = snapshot
		.bounded_path_between(BoundedPathSetRequest {
			from: &from_endpoints,
			to: &to_endpoints,
			relations: &query.relation,
			avoid: &[],
			limits: BoundedPathLimits {
				max_depth: query.max_depth,
				max_symbols: query.max_symbols,
				max_edges: query.max_edges,
			},
			scope: &path_scope,
		})
		.ok_or_else(|| {
			QueryError::new(
				"path_index_unavailable",
				"the linkage snapshot has no symbol ordinal index; refresh the workspace",
			)
		})?;
	let found = from_endpoints.intersects(&to_endpoints) || !search.path.is_empty();
	let assessment = graph_search_assessment(
		&search.coverage,
		GraphSearchLimitStatus {
			operation: GraphSearchOperation::Path,
			max_depth: query.max_depth,
			depth_reached: search.depth_reached,
			max_symbols: query.max_symbols,
			explored_symbols: search.explored_symbols,
			max_edges: query.max_edges,
			admitted_references: search.coverage.total,
			depth_limit_reached: search.depth_limit_reached,
			symbol_limit_reached: search.symbol_limit_reached,
			edge_limit_reached: search.edge_limit_reached,
		},
		query.min_coverage,
	);
	let complete = assessment.complete;
	let (reachable, no_path, verdict) = graph_path_truth(found, complete, query.expect);
	let path = search
		.path
		.iter()
		.map(|edge| graph_path_step(snapshot, roots, edge))
		.collect::<Result<Vec<_>, _>>()?;
	let result = GraphPathResult {
		from: symbol_dto(from, from_source, roots),
		to: symbol_dto(to, to_source, roots),
		from_endpoint_symbols: from_endpoints.len(),
		to_endpoint_symbols: to_endpoints.len(),
		expectation: query.expect,
		verdict,
		reachable,
		no_path,
		path,
		coverage: assessment.coverage,
		search: GraphPathSearchStats {
			max_depth: query.max_depth,
			max_symbols: query.max_symbols,
			max_edges: query.max_edges,
			depth_reached: search.depth_reached,
			explored_symbols: search.explored_symbols,
			explored_edges: search.explored_edges,
			admitted_references: search.coverage.total,
			depth_limit_reached: search.depth_limit_reached,
			symbol_limit_reached: search.symbol_limit_reached,
			edge_limit_reached: search.edge_limit_reached,
		},
		reasons: assessment.reasons,
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::GraphPath(Box::new(result)),
		next_cursor: None,
	})
}

fn natural_graph_endpoint_sets(
	snapshot: &WorkspaceSnapshot,
	from: &SymbolRecord,
	to: &SymbolRecord,
	max_symbols: usize,
	operation: &str,
) -> Result<(SymbolSet, SymbolSet), QueryError> {
	if from.id == to.id {
		let endpoint = snapshot
			.index
			.inventory
			.catalog()
			.ordinal(&from.id)
			.map(SymbolSet::from_symbol)
			.unwrap_or_default();
		return Ok((endpoint.clone(), endpoint));
	}
	let from_endpoints = natural_graph_endpoints(snapshot, from, max_symbols)
		.map_err(|total| graph_endpoint_budget_error(operation, "from", total, max_symbols))?;
	let to_endpoints = natural_graph_endpoints(snapshot, to, max_symbols)
		.map_err(|total| graph_endpoint_budget_error(operation, "to", total, max_symbols))?;
	Ok((from_endpoints, to_endpoints))
}

fn graph_endpoint_budget_error(
	operation: &str,
	endpoint: &str,
	total: usize,
	max_symbols: usize,
) -> QueryError {
	let next = if total <= code_moniker_query::MAX_GRAPH_SYMBOLS {
		format!(
			"increase max_symbols to at least {total} or select a more specific member endpoint"
		)
	} else {
		format!(
			"select a more specific member endpoint; this natural endpoint scope exceeds the protocol ceiling of {} symbols",
			code_moniker_query::MAX_GRAPH_SYMBOLS
		)
	};
	QueryError::new(
		"graph_scope_too_large",
		format!(
			"{operation} {endpoint} endpoint scope resolves naturally to {total} owner-and-descendant symbols, above max_symbols={max_symbols}; next:{next}",
		),
	)
}

fn natural_graph_endpoints(
	snapshot: &WorkspaceSnapshot,
	owner: &SymbolRecord,
	max_symbols: usize,
) -> Result<SymbolSet, usize> {
	let inventory = &snapshot.index.inventory;
	let shape = Shape::for_kind(owner.kind.as_bytes());
	if !matches!(shape, Shape::Type | Shape::Namespace) {
		return Ok(inventory
			.catalog()
			.ordinal(&owner.id)
			.map(SymbolSet::from_symbol)
			.unwrap_or_default());
	}
	inventory.owner_and_descendants_bounded(&owner.id, max_symbols)
}

fn graph_path_truth(
	found: bool,
	complete: bool,
	expectation: GraphPathExpectation,
) -> (Option<bool>, Option<bool>, GraphPathVerdict) {
	if found {
		return (
			Some(true),
			Some(false),
			if expectation == GraphPathExpectation::Reachable {
				GraphPathVerdict::Pass
			} else {
				GraphPathVerdict::Fail
			},
		);
	}
	if !complete {
		return (None, None, GraphPathVerdict::Inconclusive);
	}
	(
		Some(false),
		Some(true),
		if expectation == GraphPathExpectation::NoPath {
			GraphPathVerdict::Pass
		} else {
			GraphPathVerdict::Fail
		},
	)
}

fn push_path_gap_reason(reasons: &mut Vec<String>, reason: &str, count: usize) {
	if count > 0 {
		reasons.push(format!("{reason}:{count}"));
	}
}

#[derive(Debug)]
pub(crate) struct GraphSearchAssessment {
	pub(crate) complete: bool,
	pub(crate) coverage: GraphPathCoverage,
	pub(crate) reasons: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum GraphSearchOperation {
	Path,
	Corridor,
}

#[derive(Clone, Copy)]
pub(crate) struct GraphSearchLimitStatus {
	pub(crate) operation: GraphSearchOperation,
	pub(crate) max_depth: usize,
	pub(crate) depth_reached: usize,
	pub(crate) max_symbols: usize,
	pub(crate) explored_symbols: usize,
	pub(crate) max_edges: usize,
	pub(crate) admitted_references: usize,
	pub(crate) depth_limit_reached: bool,
	pub(crate) symbol_limit_reached: bool,
	pub(crate) edge_limit_reached: bool,
}

pub(crate) fn graph_search_assessment(
	coverage: &BoundedPathCoverage,
	limits: GraphSearchLimitStatus,
	min_coverage: usize,
) -> GraphSearchAssessment {
	let coverage_percent = coverage.percent();
	let internal_gap = coverage.gap_reasons.contains_key("missing_symbol_ordinal")
		|| coverage
			.gap_reasons
			.contains_key("missing_reference_record");
	let complete = !limits.depth_limit_reached
		&& !limits.symbol_limit_reached
		&& !limits.edge_limit_reached
		&& !internal_gap
		&& coverage_percent >= min_coverage;
	let mut reasons = Vec::new();
	if limits.depth_limit_reached {
		let next = match (
			limits.operation,
			limits.max_depth >= code_moniker_query::MAX_GRAPH_DEPTH,
		) {
			(GraphSearchOperation::Path, true) => "narrow relation/workspace",
			(GraphSearchOperation::Path, false) => {
				"increase max_depth or narrow relation/workspace"
			}
			(GraphSearchOperation::Corridor, true) => "narrow path/lang/kind/shape/srcset",
			(GraphSearchOperation::Corridor, false) => {
				"increase max_depth or narrow path/lang/kind/shape/srcset"
			}
		};
		reasons.push(format!(
			"depth_limit:reached={},max={},next={next}",
			limits.depth_reached, limits.max_depth,
		));
	}
	if limits.symbol_limit_reached {
		let next = match (
			limits.operation,
			limits.max_symbols >= code_moniker_query::MAX_GRAPH_SYMBOLS,
		) {
			(GraphSearchOperation::Path, true) => "narrow relation/workspace",
			(GraphSearchOperation::Path, false) => {
				"increase max_symbols or narrow relation/workspace"
			}
			(GraphSearchOperation::Corridor, true) => "narrow path/lang/kind/shape/srcset",
			(GraphSearchOperation::Corridor, false) => {
				"narrow path/lang/kind/shape/srcset or increase max_symbols"
			}
		};
		reasons.push(format!(
			"symbol_limit:used={},max={},next={next}",
			limits.explored_symbols, limits.max_symbols,
		));
	}
	if limits.edge_limit_reached {
		let next = match (
			limits.operation,
			limits.max_edges >= code_moniker_query::MAX_GRAPH_EDGES,
		) {
			(GraphSearchOperation::Path, true) => "narrow relation/workspace",
			(GraphSearchOperation::Path, false) => {
				"increase max_edges or narrow relation/workspace"
			}
			(GraphSearchOperation::Corridor, _) => "narrow relation or path/lang/kind/shape/srcset",
		};
		reasons.push(format!(
			"edge_limit:used={},max={},next={next}",
			limits.admitted_references, limits.max_edges,
		));
	}
	if coverage_percent < min_coverage {
		reasons.push(format!(
			"coverage_below_threshold:actual={coverage_percent},minimum={min_coverage},next=lower min_coverage if the emitted result is sufficient"
		));
	}
	for (reason, count) in &coverage.gap_reasons {
		push_path_gap_reason(&mut reasons, reason, *count);
	}
	GraphSearchAssessment {
		complete,
		coverage: GraphPathCoverage {
			total: coverage.total,
			decided: coverage.decided,
			resolved: coverage.resolved,
			external: coverage.external,
			candidate: coverage.candidate,
			dynamic: coverage.dynamic,
			manifest_blocked: coverage.manifest_blocked,
			unresolved: coverage.unresolved,
			percent: coverage_percent,
			gap_reasons: coverage.gap_reasons.clone(),
		},
		reasons,
	}
}

fn graph_path_step(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	edge: &code_moniker_workspace::snapshot::BoundedPathEdge,
) -> Result<GraphPathStep, QueryError> {
	let view = WorkspaceView::new(snapshot);
	let symbols = view.symbols();
	let sources = view.sources();
	let references = view.references();
	let source_symbol = symbols
		.find(&edge.source)
		.ok_or_else(|| QueryError::new("symbol_not_found", "path source symbol not found"))?;
	let target_symbol = symbols
		.find(&edge.target)
		.ok_or_else(|| QueryError::new("symbol_not_found", "path target symbol not found"))?;
	let reference = references
		.reference(&edge.reference)
		.ok_or_else(|| QueryError::new("reference_not_found", "path reference not found"))?;
	let source = sources
		.record(&reference.source)
		.ok_or_else(|| QueryError::new("source_not_found", "path reference source not found"))?;
	let source_symbol_source = sources
		.record(&source_symbol.source)
		.ok_or_else(|| QueryError::new("source_not_found", "path source source not found"))?;
	let target_symbol_source = sources
		.record(&target_symbol.source)
		.ok_or_else(|| QueryError::new("source_not_found", "path target source not found"))?;
	Ok(GraphPathStep {
		source: symbol_dto(source_symbol, source_symbol_source, roots),
		target: symbol_dto(target_symbol, target_symbol_source, roots),
		relation: reference.kind.clone(),
		reference: reference.id.to_string(),
		file: source.rel_path.clone(),
		line_range: reference.line_range,
	})
}

pub(crate) fn graph_corridor_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: GraphCorridorQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let symbol_scope = symbol_scope_for_roots(snapshot, roots, &selected_roots);
	let from = find_symbol(snapshot, &symbol_scope, &query.from)?;
	let to = find_symbol(snapshot, &symbol_scope, &query.to)?;
	let view = WorkspaceView::new(snapshot);
	let sources = view.sources();
	let from_source = sources
		.record(&from.source)
		.ok_or_else(|| QueryError::new("source_not_found", "source symbol source not found"))?;
	let to_source = sources
		.record(&to.source)
		.ok_or_else(|| QueryError::new("source_not_found", "target symbol source not found"))?;
	for (uri, source) in [(&query.from, from_source), (&query.to, to_source)] {
		if source_root(roots, &selected_roots, source).is_none() {
			return Err(QueryError::new(
				"symbol_not_in_workspace",
				format!("symbol {uri} is not in the selected workspace"),
			));
		}
	}
	let (from_endpoints, to_endpoints) =
		natural_graph_endpoint_sets(snapshot, from, to, query.max_symbols, "corridor")?;
	let combined = from_endpoints.union_len(&to_endpoints);
	if combined > query.max_symbols {
		return Err(graph_endpoint_budget_error(
			"corridor",
			"combined",
			combined,
			query.max_symbols,
		));
	}
	let endpoint_symbols = from_endpoints.union(&to_endpoints);
	let scope = graph_corridor_scope(
		snapshot,
		roots,
		&selected_roots,
		&query.scope,
		&endpoint_symbols,
		query.max_symbols,
	)?;
	let search = snapshot
		.bounded_corridor_between(BoundedCorridorSetRequest {
			from: &from_endpoints,
			to: &to_endpoints,
			relations: &query.relation,
			limits: BoundedPathLimits {
				max_depth: query.max_depth,
				max_symbols: query.max_symbols,
				max_edges: query.max_edges,
			},
			scope: &scope,
		})
		.ok_or_else(|| {
			QueryError::new(
				"corridor_index_unavailable",
				"the linkage snapshot has no symbol ordinal index; refresh the workspace",
			)
		})?;
	let assessment = graph_search_assessment(
		&search.coverage,
		GraphSearchLimitStatus {
			operation: GraphSearchOperation::Corridor,
			max_depth: query.max_depth,
			depth_reached: search
				.forward_depth_reached
				.max(search.reverse_depth_reached),
			max_symbols: query.max_symbols,
			explored_symbols: search.explored_symbols,
			max_edges: query.max_edges,
			admitted_references: search.coverage.total,
			depth_limit_reached: search.depth_limit_reached,
			symbol_limit_reached: search.symbol_limit_reached,
			edge_limit_reached: search.edge_limit_reached,
		},
		query.min_coverage,
	);
	let search_complete = assessment.complete;
	let established = from_endpoints.intersects(&to_endpoints) || !search.members.is_empty();
	let connected = if established {
		Some(true)
	} else if search_complete {
		Some(false)
	} else {
		None
	};
	let member_count = search.members.len();
	let edge_count = search
		.edges
		.chunk_by(|left, right| left.source == right.source && left.target == right.target)
		.count();
	let members = search
		.members
		.iter()
		.map(|id| {
			let symbol = view
				.symbols()
				.find(id)
				.ok_or_else(|| QueryError::new("symbol_not_found", "corridor member not found"))?;
			let source = sources.record(&symbol.source).ok_or_else(|| {
				QueryError::new("source_not_found", "corridor member source not found")
			})?;
			Ok(symbol_dto(symbol, source, roots))
		})
		.collect::<Result<Vec<_>, QueryError>>()?;
	let references = view.references();
	let edges = search
		.edges
		.chunk_by(|left, right| left.source == right.source && left.target == right.target)
		.map(|group| {
			let mut relations = BTreeSet::new();
			for edge in group {
				let reference = references.reference(&edge.reference).ok_or_else(|| {
					QueryError::new("reference_not_found", "corridor reference not found")
				})?;
				relations.insert(reference.kind.clone());
			}
			let representative = graph_path_step(snapshot, roots, &group[0])?;
			Ok(GraphCorridorEdge {
				source: representative.source,
				target: representative.target,
				relations: relations.into_iter().collect(),
				count: group.len(),
				representative_reference: representative.reference,
				representative_file: representative.file,
				representative_line_range: representative.line_range,
			})
		})
		.collect::<Result<Vec<_>, QueryError>>()?;
	let result = GraphCorridorResult {
		from: symbol_dto(from, from_source, roots),
		to: symbol_dto(to, to_source, roots),
		from_endpoint_symbols: from_endpoints.len(),
		to_endpoint_symbols: to_endpoints.len(),
		member_count,
		edge_count,
		members,
		edges,
		connected,
		result_complete: true,
		search_complete,
		coverage: assessment.coverage,
		search: GraphCorridorSearchStats {
			max_depth: query.max_depth,
			max_symbols: query.max_symbols,
			max_edges: query.max_edges,
			forward_depth_reached: search.forward_depth_reached,
			reverse_depth_reached: search.reverse_depth_reached,
			explored_symbols: search.explored_symbols,
			explored_edges: search.explored_edges,
			admitted_references: search.coverage.total,
			depth_limit_reached: search.depth_limit_reached,
			symbol_limit_reached: search.symbol_limit_reached,
			edge_limit_reached: search.edge_limit_reached,
		},
		reasons: assessment.reasons,
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::GraphCorridor(Box::new(result)),
		next_cursor: None,
	})
}

fn graph_corridor_scope(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	selected_roots: &[&PathBuf],
	scope: &GraphSymbolScope,
	endpoints: &SymbolSet,
	max_symbols: usize,
) -> Result<BoundedCorridorScope, QueryError> {
	let inventory = &snapshot.index.inventory;
	if endpoints.len() == 1 {
		return BoundedCorridorScope::from_symbols(endpoints.clone(), max_symbols).ok_or_else(
			|| {
				QueryError::new(
					"graph_scope_too_large",
					"corridor scope is empty or exceeds max_symbols",
				)
			},
		);
	}
	if max_symbols < endpoints.len() {
		let next = if endpoints.len() <= code_moniker_query::MAX_GRAPH_SYMBOLS {
			format!("increase max_symbols to at least {}", endpoints.len())
		} else {
			format!(
				"select more specific member endpoints; the endpoint scope exceeds the protocol ceiling of {} symbols",
				code_moniker_query::MAX_GRAPH_SYMBOLS
			)
		};
		return Err(QueryError::new(
			"graph_scope_too_large",
			format!(
				"corridor owner endpoints expand naturally to {} symbols, above max_symbols={max_symbols}; next:{next}",
				endpoints.len(),
			),
		));
	}
	let facets = inventory.facets();
	let path_filter = FilePathFilter::compile(&scope.path)
		.map_err(|error| QueryError::new("invalid_graph_scope", error.to_string()))?;
	let all_roots_selected = selected_roots.len() == roots.len();
	let mut selected_root_indices = roots
		.iter()
		.enumerate()
		.filter(|(_, root)| {
			selected_roots
				.iter()
				.any(|selected| selected.as_path() == root.as_path())
		})
		.map(|(index, _)| index)
		.collect::<Vec<_>>();
	if all_roots_selected {
		selected_root_indices.push(roots.len());
	}
	let mut facet_groups = Vec::new();
	let mut facet_matches = Vec::new();
	push_lazy_scope_facet(
		&mut facet_groups,
		&mut facet_matches,
		"lang",
		&scope.lang,
		|value| facets.symbols_by_language(value),
	);
	push_lazy_scope_facet(
		&mut facet_groups,
		&mut facet_matches,
		"kind",
		&scope.kind,
		|value| facets.symbols_by_kind(value),
	);
	push_lazy_scope_facet(
		&mut facet_groups,
		&mut facet_matches,
		"shape",
		&scope.shape,
		|value| facets.symbols_by_shape(value),
	);
	push_lazy_scope_facet(
		&mut facet_groups,
		&mut facet_matches,
		"srcset",
		&scope.srcset,
		|value| facets.symbols_by_srcset(value),
	);
	if scope.path.is_empty() && facet_groups.is_empty() {
		return Err(QueryError::new(
			"invalid_graph_scope",
			"at least one semantic scope is required: path, lang, kind, shape, or srcset",
		));
	}
	let workspace_upper_bound = selected_root_indices
		.iter()
		.filter_map(|root| facets.symbols_by_source_root(*root))
		.fold(0usize, |total, posting| total.saturating_add(posting.len()));
	facet_matches.push(("workspace", workspace_upper_bound, false));
	let best_known_anchor = facet_groups
		.iter()
		.map(|group| group.upper_bound)
		.chain(std::iter::once(workspace_upper_bound))
		.min()
		.unwrap_or(workspace_upper_bound);
	let path_cap = best_known_anchor.min(max_symbols.saturating_add(1));
	let path_upper_bound = (!scope.path.is_empty()).then(|| {
		let mut count = 0usize;
		if path_cap > 0 {
			for (_, posting) in facets
				.source_path_postings()
				.filter(|(path, _)| path_filter.matches(std::path::Path::new(path)))
			{
				count = count.saturating_add(posting.len());
				if count >= path_cap {
					count = path_cap;
					break;
				}
			}
		}
		(count, count == path_cap)
	});
	if let Some((count, capped)) = path_upper_bound {
		facet_matches.push(("path", count, capped));
	}
	let mut anchor = facet_groups
		.iter()
		.enumerate()
		.map(|(index, group)| (group.upper_bound, ScopeAnchor::Facet(index)))
		.chain(path_upper_bound.map(|(count, _)| (count, ScopeAnchor::Path)))
		.chain(std::iter::once((
			workspace_upper_bound,
			ScopeAnchor::Workspace,
		)))
		.min_by_key(|(count, _)| *count)
		.map(|(_, anchor)| anchor);
	let mut symbols = SymbolSet::new();
	let mut visit = |ordinal| {
		if symbols.contains(ordinal) {
			return true;
		}
		let Some(record) = inventory.record(ordinal) else {
			return true;
		};
		if !selected_root_indices.contains(&record.source_root)
			|| (!scope.path.is_empty()
				&& !path_filter.matches(std::path::Path::new(record.source_path.as_ref())))
			|| facet_groups.iter().any(|group| !group.matches(ordinal))
		{
			return true;
		}
		symbols.insert(ordinal);
		symbols.len() <= max_symbols
	};
	match anchor.take() {
		Some(ScopeAnchor::Facet(index)) => {
			'candidates: for posting in &facet_groups[index].postings {
				for ordinal in posting.iter() {
					if !visit(ordinal) {
						break 'candidates;
					}
				}
			}
		}
		Some(ScopeAnchor::Path) => {
			'candidates: for (_, posting) in facets
				.source_path_postings()
				.filter(|(path, _)| path_filter.matches(std::path::Path::new(path)))
			{
				for ordinal in posting.iter() {
					if !visit(ordinal) {
						break 'candidates;
					}
				}
			}
		}
		Some(ScopeAnchor::Workspace) => {
			'candidates: for posting in selected_root_indices
				.iter()
				.filter_map(|root| facets.symbols_by_source_root(*root))
			{
				for ordinal in posting.iter() {
					if !visit(ordinal) {
						break 'candidates;
					}
				}
			}
		}
		None => unreachable!("workspace always provides a scope anchor"),
	}
	for endpoint in endpoints.iter() {
		symbols.insert(endpoint);
	}
	if symbols.len() > max_symbols {
		let matches = facet_matches
			.iter()
			.map(|(facet, count, capped)| {
				format!("{facet}{}{count}", if *capped { ">=" } else { "=" })
			})
			.collect::<Vec<_>>()
			.join(",");
		let narrow = facet_matches
			.iter()
			.min_by_key(|(_, count, _)| *count)
			.map_or_else(
				|| "add a selective path/kind/shape/srcset facet".to_string(),
				|(facet, count, _)| {
					if *facet == "workspace" {
						"add a selective path/kind/shape/srcset facet".to_string()
					} else {
						format!(
							"narrow {facet} (currently {count} matches) or add another selective facet"
						)
					}
				},
			);
		let next = if symbols.len() <= code_moniker_query::MAX_GRAPH_SYMBOLS {
			format!(
				"increase max_symbols above at least {} observed matches or {narrow}",
				symbols.len()
			)
		} else {
			format!(
				"{narrow}; this semantic scope exceeds the protocol ceiling of {} symbols",
				code_moniker_query::MAX_GRAPH_SYMBOLS
			)
		};
		return Err(QueryError::new(
			"graph_scope_too_large",
			format!(
				"semantic scope matched at least {} symbols including endpoints, above max_symbols={max_symbols}; facet_matches:{matches}; next:{next}",
				symbols.len(),
			),
		));
	}
	BoundedCorridorScope::from_symbols(symbols, max_symbols).ok_or_else(|| {
		QueryError::new(
			"invalid_graph_scope",
			"corridor scope must contain at least one bounded symbol",
		)
	})
}

struct LazyScopeFacet<'a> {
	postings: Vec<&'a SymbolSet>,
	upper_bound: usize,
}

#[derive(Clone, Copy)]
enum ScopeAnchor {
	Facet(usize),
	Path,
	Workspace,
}

impl LazyScopeFacet<'_> {
	fn matches(&self, ordinal: code_moniker_workspace::snapshot::SymbolOrdinal) -> bool {
		self.postings
			.iter()
			.any(|posting| posting.contains(ordinal))
	}
}

fn push_lazy_scope_facet<'a>(
	facets: &mut Vec<LazyScopeFacet<'a>>,
	facet_matches: &mut Vec<(&'static str, usize, bool)>,
	name: &'static str,
	values: &[String],
	mut posting: impl FnMut(&str) -> Option<&'a SymbolSet>,
) {
	if values.is_empty() {
		return;
	}
	let postings = values
		.iter()
		.filter_map(|value| posting(value))
		.collect::<Vec<_>>();
	push_lazy_scope_postings(facets, facet_matches, name, postings);
}

fn push_lazy_scope_postings<'a>(
	facets: &mut Vec<LazyScopeFacet<'a>>,
	facet_matches: &mut Vec<(&'static str, usize, bool)>,
	name: &'static str,
	postings: Vec<&'a SymbolSet>,
) {
	let upper_bound = postings
		.iter()
		.fold(0usize, |total, posting| total.saturating_add(posting.len()));
	facet_matches.push((name, upper_bound, false));
	facets.push(LazyScopeFacet {
		postings,
		upper_bound,
	});
}

pub(super) fn resolve_unit_boundary(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	selected_roots: &[&PathBuf],
	symbol_scope: &SymbolSet,
	focus: &str,
) -> Result<(UnitBoundary, SymbolGraphFocus), QueryError> {
	let symbol_error = match find_symbol(snapshot, symbol_scope, focus) {
		Ok(symbol) => {
			let source = WorkspaceView::new(snapshot)
				.sources()
				.record(&symbol.source)
				.ok_or_else(|| QueryError::new("source_not_found", "focus source not found"))?;
			return Ok((
				UnitBoundary::IdentityPrefix {
					prefix: symbol.identity.to_string(),
					slot: symbol.id.file(),
				},
				SymbolGraphFocus::Symbol {
					symbol: Box::new(symbol_dto(symbol, source, roots)),
				},
			));
		}
		Err(error) => error,
	};
	let source = snapshot.index.sources.iter().find(|source| {
		source.rel_path == focus && source_root(roots, selected_roots, source).is_some()
	});
	let Some(source) = source else {
		if symbol_error.code == "symbol_ambiguous" {
			return Err(symbol_error);
		}
		return Err(directory_or_unknown_focus_error(snapshot, focus));
	};
	Ok((
		UnitBoundary::File {
			source: source.id,
			slot: source.id.file(),
		},
		SymbolGraphFocus::File {
			path: source.rel_path.clone(),
		},
	))
}

pub(super) fn navigable_anchor<'a>(
	symbols: &code_moniker_workspace::snapshot::SymbolView<'a>,
	id: SymbolId,
) -> Option<&'a SymbolRecord> {
	let mut current = symbols.find(&id)?;
	loop {
		if current.navigable {
			return Some(current);
		}
		let parent = current.parent?;
		current = symbols.find(&parent)?;
	}
}

pub(super) fn resolved_reference_target(
	snapshot: &WorkspaceSnapshot,
	reference: &ReferenceId,
) -> Option<SymbolId> {
	if let Some(index) = snapshot.linkage.read_index.get() {
		return index.resolved_target(reference);
	}
	snapshot
		.linkage
		.resolved
		.iter()
		.find(|edge| &edge.reference == reference)
		.map(|edge| edge.target)
}

pub(super) fn incoming_reference_ids(
	snapshot: &WorkspaceSnapshot,
	symbol: &SymbolId,
) -> Vec<ReferenceId> {
	WorkspaceView::new(snapshot)
		.references()
		.incoming_ids(symbol)
}
