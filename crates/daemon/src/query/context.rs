use code_moniker_query::{
	ChangeContextCoverageDto, ChangeContextQuery, ChangeContextResult, ChangeReviewFile,
	ChangeReviewQuery, ChangeReviewSymbol, NoteDto, NotesAction, NotesQuery, Page, QueryError,
	QueryResponse, QueryResult, RuleApplicabilityDto, RulesApplicableQuery, SourceSnippet,
	SymbolGraphFocus, SymbolGraphQuery, SymbolGraphResult, WorkspaceGeneration,
};
use code_moniker_workspace::snapshot::WorkspaceSnapshot;
use code_moniker_workspace::source::LocalResourceCache;

use super::changes::change_review_response;
use super::graph::symbol_graph_response;
use super::model::ResponseContext;
use super::notes::notes_response;
use super::rules::{focus_rule_coordinates, rules_applicable_response};
use super::symbols::symbol_detail_response;
use crate::daemon::WorkspaceDaemon;

pub(crate) fn change_context_response(
	daemon: &mut WorkspaceDaemon,
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	query: ChangeContextQuery,
	change_failure: Option<QueryError>,
) -> Result<QueryResponse, QueryError> {
	let max_items = query.max_items;
	let context_graph = bounded_context_graph(snapshot, response, &query, max_items)?;
	let (notes_total, notes) = context_notes(
		daemon,
		snapshot,
		&context_graph.focus,
		max_items,
		response.generation,
	)?;
	let (rules_total, rules) = context_rules(snapshot, response, &query, max_items)?;
	let (changes, acquisition_failure) = if change_failure.is_some() {
		(ContextChanges::empty(), None)
	} else {
		match context_changes(
			&daemon.cache,
			snapshot,
			response,
			query.workspace.clone(),
			&context_graph.file,
			max_items,
		) {
			Ok(changes) => (changes, None),
			Err(error) => (ContextChanges::empty(), Some(error)),
		}
	};
	let mut change_dependency = crate::runtime_dependencies::git_change_dependency(
		query.workspace.as_deref(),
		response.roots,
		daemon.process_scope,
	)?;
	let change_failure = change_failure.or(acquisition_failure);
	if let Some(failure) = &change_failure {
		crate::runtime_dependencies::apply_change_failure(&mut change_dependency, failure);
	}
	let profile_arg = query
		.profile
		.as_deref()
		.map_or_else(String::new, |profile| {
			format!(" profile=\"{}\"", profile.replace('"', "\\\""))
		});
	let escaped_file = context_graph.file.replace('"', "\\\"");
	let suggested_checks = vec![format!(
		"code_moniker_rules uri=\"workspace\" action=\"run\"{profile_arg} file=\"{escaped_file}\" limit=20"
	)];
	let coverage = ChangeContextCoverageDto {
		members_total: context_graph.members_total,
		members_emitted: context_graph.graph.members.len(),
		internal_edges_total: context_graph.internal_edges_total,
		internal_edges_emitted: context_graph.graph.internal_edges.len(),
		callers_total: context_graph.callers_total,
		callers_emitted: context_graph.graph.callers.len(),
		callees_total: context_graph.callees_total,
		callees_emitted: context_graph.graph.callees.len(),
		notes_total,
		notes_emitted: notes.len(),
		rules_total,
		rules_emitted: rules.len(),
		changes_total: changes.total,
		changes_emitted: changes.files.len() + changes.symbols.len(),
	};
	Ok(QueryResponse {
		generation: response.generation,
		result: QueryResult::ChangeContext(Box::new(ChangeContextResult {
			focus: context_graph.focus,
			source: context_graph.source,
			graph: Box::new(context_graph.graph),
			notes,
			rules,
			changed_files: changes.files,
			changed_symbols: changes.symbols,
			change_dependency,
			suggested_checks,
			coverage,
		})),
		next_cursor: None,
	})
}

struct BoundedContextGraph {
	graph: SymbolGraphResult,
	focus: SymbolGraphFocus,
	file: String,
	source: Option<SourceSnippet>,
	members_total: usize,
	internal_edges_total: usize,
	callers_total: usize,
	callees_total: usize,
}

fn bounded_context_graph(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	query: &ChangeContextQuery,
	max_items: usize,
) -> Result<BoundedContextGraph, QueryError> {
	let graph_response = symbol_graph_response(
		snapshot,
		response.roots,
		SymbolGraphQuery {
			workspace: query.workspace.clone(),
			focus: query.focus.clone(),
			..Default::default()
		},
		response.generation,
	)?;
	let QueryResult::SymbolGraph(graph) = graph_response.result else {
		return Err(QueryError::new(
			"graph_contract",
			"unexpected symbol graph response",
		));
	};
	let mut graph = *graph;
	let members_total = graph.coverage.members.total;
	let internal_edges_total = graph.coverage.internal_edges.total;
	let callers_total = graph.coverage.callers.total;
	let callees_total = graph.coverage.callees.total;
	graph.callers.truncate(max_items);
	graph.callees.truncate(max_items);
	graph.members.truncate(max_items);
	graph.internal_edges.truncate(max_items);
	graph.coverage.members.returned = graph.members.len();
	graph.coverage.internal_edges.returned = graph.internal_edges.len();
	graph.coverage.callers.returned = graph.callers.len();
	graph.coverage.callees.returned = graph.callees.len();
	let focus = graph.focus.clone();
	let (file, _, _) = focus_rule_coordinates(snapshot, &focus)?;
	let source = match &focus {
		SymbolGraphFocus::Symbol { symbol } => {
			let detail = symbol_detail_response(
				snapshot,
				response.roots,
				query.workspace.as_deref(),
				&symbol.uri,
				2,
				response.generation,
			)?;
			match detail.result {
				QueryResult::SymbolDetail(detail) => detail.source,
				_ => None,
			}
		}
		SymbolGraphFocus::File { .. } => None,
	};
	Ok(BoundedContextGraph {
		graph,
		focus,
		file,
		source,
		members_total,
		internal_edges_total,
		callers_total,
		callees_total,
	})
}

fn context_notes(
	daemon: &mut WorkspaceDaemon,
	snapshot: &WorkspaceSnapshot,
	focus: &SymbolGraphFocus,
	max_items: usize,
	generation: Option<WorkspaceGeneration>,
) -> Result<(usize, Vec<NoteDto>), QueryError> {
	Ok(match focus {
		SymbolGraphFocus::Symbol { symbol } => {
			let notes = notes_response(
				daemon,
				snapshot,
				NotesQuery {
					action: NotesAction::List,
					id: None,
					moniker: Some(symbol.uri.clone()),
					kind: None,
					status: None,
					title: None,
					body: None,
					created_by: None,
					orphan: None,
					include_done: false,
				},
				Page {
					cursor: None,
					limit: max_items,
				},
				generation,
			)?;
			match notes.result {
				QueryResult::Notes(notes) => (notes.total, notes.rows),
				_ => (0, Vec::new()),
			}
		}
		SymbolGraphFocus::File { .. } => (0, Vec::new()),
	})
}

fn context_rules(
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	query: &ChangeContextQuery,
	max_items: usize,
) -> Result<(usize, Vec<RuleApplicabilityDto>), QueryError> {
	let applicable = rules_applicable_response(
		snapshot,
		response,
		RulesApplicableQuery {
			workspace: query.workspace.clone(),
			focus: query.focus.clone(),
			profile: query.profile.clone(),
			rules: None,
		},
		Page {
			cursor: None,
			limit: usize::MAX,
		},
	)?;
	let QueryResult::RulesApplicable(applicable) = applicable.result else {
		return Err(QueryError::new(
			"rules_contract",
			"unexpected applicable rules response",
		));
	};
	let total = applicable
		.rows
		.iter()
		.filter(|row| row.status == "applicable")
		.count();
	let rows = applicable
		.rows
		.into_iter()
		.filter(|row| row.status == "applicable")
		.take(max_items)
		.collect::<Vec<_>>();
	Ok((total, rows))
}

struct ContextChanges {
	total: usize,
	files: Vec<ChangeReviewFile>,
	symbols: Vec<ChangeReviewSymbol>,
}

impl ContextChanges {
	fn empty() -> Self {
		Self {
			total: 0,
			files: Vec::new(),
			symbols: Vec::new(),
		}
	}
}

fn context_changes(
	cache: &LocalResourceCache,
	snapshot: &WorkspaceSnapshot,
	response: ResponseContext<'_>,
	workspace: Option<String>,
	file: &str,
	max_items: usize,
) -> Result<ContextChanges, QueryError> {
	let review = change_review_response(
		cache,
		snapshot,
		response.roots,
		ChangeReviewQuery { workspace },
		response.generation,
	)?;
	let QueryResult::ChangeReview(review) = review.result else {
		return Err(QueryError::new(
			"change_contract",
			"unexpected change review response",
		));
	};
	let all_changed_files = review
		.files
		.iter()
		.filter(|changed| {
			changed.old_path.as_deref() == Some(file) || changed.new_path.as_deref() == Some(file)
		})
		.cloned()
		.collect::<Vec<_>>();
	let all_changed_symbols = review
		.symbol_changes
		.iter()
		.filter(|changed| {
			changed.old.as_ref().is_some_and(|side| side.file == file)
				|| changed.new.as_ref().is_some_and(|side| side.file == file)
		})
		.cloned()
		.collect::<Vec<_>>();
	let changes_total = all_changed_files.len() + all_changed_symbols.len();
	let changed_files = all_changed_files
		.into_iter()
		.take(max_items)
		.collect::<Vec<_>>();
	let changed_symbols = all_changed_symbols
		.into_iter()
		.take(max_items.saturating_sub(changed_files.len()))
		.collect::<Vec<_>>();
	Ok(ContextChanges {
		total: changes_total,
		files: changed_files,
		symbols: changed_symbols,
	})
}
