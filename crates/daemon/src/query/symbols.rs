use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::graph::resolved_reference_target;
use super::model::UsageDtoContext;
use crate::helpers::{
	DEFAULT_SCHEME, change_counts_by_source, find_symbol, path_prefix, selected_roots,
	sorted_counts, source_root, source_snippet, symbol_dto, symbol_scope_for_roots,
	symbol_search_dto, usage_dto, workspace_label_from_paths,
};
use crate::pagination::page_rows;
use crate::views;
use code_moniker_core::core::shape::Shape;
use code_moniker_query::{
	CountDto, Page, QueryError, QueryResponse, QueryResult, SymbolDetailResult, SymbolDto,
	SymbolInsightsResult, SymbolListResult, SymbolSearchQuery, SymbolUsagesQuery,
	SymbolUsagesResult, TreeChildrenQuery, TreeChildrenResult, TreeNode, TreeNodeKind,
	UsageDirection, UsageDto, UsageSummaryDto, ViewReadQuery, WorkspaceGeneration,
	symbol_is_test_artifact,
};
use code_moniker_workspace::glob::FilePathFilter;
use code_moniker_workspace::snapshot::{
	ReferenceId, SymbolId, SymbolRecord, SymbolSet, WorkspaceSnapshot, WorkspaceView,
};

pub(crate) fn tree_children_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: TreeChildrenQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let plain_scope = tree_plain_scope(&query.path);
	let prefix = plain_scope.as_deref().unwrap_or_default();
	let mut map = BTreeMap::<String, TreeNode>::new();
	let mut scoped_sources = Vec::new();
	let change_counts = change_counts_by_source(snapshot);
	for source in &snapshot.index.sources {
		let Some(root) = source_root(roots, &selected_roots, source) else {
			continue;
		};
		if !query.lang.is_empty() && !query.lang.iter().any(|lang| lang == &source.language) {
			continue;
		}
		if !path_filter.matches(&source.rel_path) {
			continue;
		}
		scoped_sources.push((root, source));
		let exact_file_scope = plain_scope
			.as_deref()
			.is_some_and(|scope| source.rel_path == scope);
		let remainder = if exact_file_scope {
			source.rel_path.as_str()
		} else {
			source.rel_path[prefix.len()..].trim_start_matches('/')
		};
		if remainder.is_empty() {
			continue;
		}
		let parts = remainder.split('/').collect::<Vec<_>>();
		let depth = query.depth.max(1);
		let take = parts.len().min(depth);
		let row_path = if exact_file_scope || prefix.is_empty() {
			parts[..take].join("/")
		} else {
			format!(
				"{}/{}",
				prefix.trim_end_matches('/'),
				parts[..take].join("/")
			)
		};
		let kind = if take < parts.len() {
			TreeNodeKind::Directory
		} else {
			TreeNodeKind::File
		};
		let root_label = root.display().to_string();
		let entry_key = format!("{root_label}\0{row_path}");
		let entry = map.entry(entry_key).or_insert_with(|| TreeNode {
			root: root_label,
			path: row_path,
			kind,
			language: (kind == TreeNodeKind::File).then(|| source.language.clone()),
			defs: 0,
			refs: 0,
			change_count: 0,
		});
		entry.defs += snapshot
			.index
			.symbols
			.file_records(source.id.file())
			.iter()
			.filter(|symbol| symbol.navigable)
			.count();
		entry.refs += snapshot
			.index
			.references
			.file_records(source.id.file())
			.len();
		entry.change_count += change_counts.get(&source.id).copied().unwrap_or(0);
	}
	let total_files = snapshot
		.index
		.sources
		.iter()
		.filter(|source| source_root(roots, &selected_roots, source).is_some())
		.count();
	let languages = sorted_counts(
		scoped_sources
			.iter()
			.map(|(_, source)| source.language.clone()),
	);
	let prefixes = sorted_counts(
		scoped_sources
			.iter()
			.map(|(_, source)| path_prefix(&source.rel_path)),
	);
	let paged = page_rows(map.into_values().collect(), page, current_generation)?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::TreeChildren(TreeChildrenResult {
			root: workspace_label_from_paths(&selected_roots),
			roots: selected_roots
				.iter()
				.map(|root| root.display().to_string())
				.collect(),
			total: paged.total,
			rows: paged.items,
			total_files,
			scoped_files: scoped_sources.len(),
			languages,
			prefixes,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn tree_plain_scope(paths: &[String]) -> Option<String> {
	let [path] = paths else {
		return None;
	};
	if path.contains(['*', '?']) {
		None
	} else {
		Some(normalize_tree_path(path))
	}
}

fn normalize_tree_path(path: &str) -> String {
	path.trim()
		.replace('\\', "/")
		.trim_start_matches("./")
		.trim_start_matches('/')
		.trim_end_matches('/')
		.to_string()
}

pub(crate) fn symbol_search_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolSearchQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let natural_name = natural_callable_name_pattern(&query);
	let requested_name_filter = query
		.name
		.as_deref()
		.map(regex::Regex::new)
		.transpose()
		.map_err(|err| QueryError::new("invalid_name_filter", err.to_string()))?;
	let callable_name_filter = natural_name
		.as_ref()
		.map(|(_, normalized)| regex::Regex::new(normalized))
		.transpose()
		.map_err(|err| QueryError::new("invalid_name_filter", err.to_string()))?;
	let sources = WorkspaceView::new(snapshot).sources();
	let matches_scope = |symbol: &SymbolRecord| {
		let Some(source) = sources.record(&symbol.source) else {
			return false;
		};
		source_root(roots, &selected_roots, source).is_some()
			&& path_filter.matches(&source.rel_path)
			&& (query.lang.is_empty() || query.lang.iter().any(|lang| lang == &source.language))
			&& matches_kind_shape(symbol, &query)
	};
	let use_callable_fallback = callable_name_filter.as_ref().is_some_and(|_| {
		!snapshot.index.symbols.iter().any(|symbol| {
			(query.include_non_navigable || symbol.navigable)
				&& matches_scope(symbol)
				&& requested_name_filter
					.as_ref()
					.is_none_or(|regex| regex.is_match(&symbol.name))
		})
	});
	let name_filter = if use_callable_fallback {
		callable_name_filter.as_ref()
	} else {
		requested_name_filter.as_ref()
	};
	let matches_query = |symbol: &SymbolRecord| {
		matches_scope(symbol) && name_filter.is_none_or(|regex| regex.is_match(&symbol.name))
	};
	let mut rows = if let Some(text) = query.text.as_deref().filter(|text| !text.trim().is_empty())
		&& !query.include_non_navigable
	{
		let symbols = WorkspaceView::new(snapshot).symbols();
		WorkspaceView::new(snapshot)
			.search()
			.search_symbols_matching(text, usize::MAX, matches_query)
			.into_iter()
			.map(|hit| {
				let Some(symbol) = symbols.find(&hit.symbol) else {
					return Ok(None);
				};
				let Some(source) = sources.record(&symbol.source) else {
					return Ok(None);
				};
				let mut row = symbol_search_dto(symbol, source, roots, hit.score, hit.reason);
				if query.include_code {
					row.source = source_snippet(source, symbol, query.context_lines)?;
				}
				Ok(Some(row))
			})
			.collect::<Result<Vec<_>, QueryError>>()?
			.into_iter()
			.flatten()
			.collect::<Vec<_>>()
	} else {
		snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| query.include_non_navigable || symbol.navigable)
			.filter(|symbol| matches_query(symbol))
			.filter_map(|symbol| {
				let source = sources.record(&symbol.source)?;
				Some((symbol, source))
			})
			.map(|(symbol, source)| {
				let mut row = symbol_dto(symbol, source, roots);
				if query.include_code {
					row.source = source_snippet(source, symbol, query.context_lines)?;
				}
				Ok(row)
			})
			.collect::<Result<Vec<_>, QueryError>>()?
	};
	if query
		.text
		.as_deref()
		.is_none_or(|text| text.trim().is_empty())
		|| query.include_non_navigable
	{
		rows.sort_by(symbol_dto_navigation_cmp);
	}
	let paged = page_rows(rows, page, current_generation)?;
	let hint = use_callable_fallback.then(|| {
		let (original, normalized) = natural_name.expect("callable fallback has a natural pattern");
		format!(
			"no exact symbol matched `{original}`; callable names include their parameter signature, so the query was retried as `{normalized}`"
		)
	});
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolList(SymbolListResult {
			total: paged.total,
			rows: paged.items,
			hint,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn natural_callable_name_pattern(query: &SymbolSearchQuery) -> Option<(String, String)> {
	let callable_scope = (query.kind.is_empty()
		|| query
			.kind
			.iter()
			.any(|kind| Shape::for_kind(kind.as_bytes()) == Shape::Callable))
		&& (query.shape.is_empty() || query.shape.iter().any(|shape| shape == "callable"));
	if !callable_scope {
		return None;
	}
	let name = query.name.as_deref()?;
	let bare = name.strip_prefix('^')?.strip_suffix('$')?.to_string();
	if bare.is_empty()
		|| !bare
			.chars()
			.all(|character| character.is_ascii_alphanumeric() || character == '_')
	{
		return None;
	}
	Some((name.to_string(), format!("^{bare}\\(")))
}

fn symbol_dto_navigation_cmp(left: &SymbolDto, right: &SymbolDto) -> std::cmp::Ordering {
	symbol_is_test_artifact(&left.kind, &left.file, &left.uri)
		.cmp(&symbol_is_test_artifact(
			&right.kind,
			&right.file,
			&right.uri,
		))
		.then_with(|| left.file.cmp(&right.file))
		.then_with(|| left.line_range.cmp(&right.line_range))
		.then_with(|| left.uri.cmp(&right.uri))
}

fn matches_kind_shape(symbol: &SymbolRecord, query: &SymbolSearchQuery) -> bool {
	let kind_matches = query.kind.iter().any(|kind| kind == &symbol.kind);
	let shape_matches = query
		.shape
		.iter()
		.any(|shape| Shape::for_kind(symbol.kind.as_bytes()).as_str() == shape);
	if query
		.text
		.as_deref()
		.is_some_and(|text| !text.trim().is_empty())
		&& !query.kind.is_empty()
		&& !query.shape.is_empty()
	{
		return kind_matches || shape_matches;
	}
	(query.kind.is_empty() || kind_matches) && (query.shape.is_empty() || shape_matches)
}

pub(crate) fn symbol_insights_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolSearchQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let name_filter = query
		.name
		.as_ref()
		.map(|pattern| regex::Regex::new(pattern))
		.transpose()
		.map_err(|err| QueryError::new("invalid_name_filter", err.to_string()))?;
	let mut selected_sources = vec![false; snapshot.index.sources.len()];
	let mut files = 0usize;
	let mut languages = BTreeMap::<&str, usize>::new();
	for source in &snapshot.index.sources {
		let selected = source_root(roots, &selected_roots, source).is_some()
			&& path_filter.matches(&source.rel_path)
			&& (query.lang.is_empty() || query.lang.iter().any(|lang| lang == &source.language));
		if selected {
			if let Some(slot) = selected_sources.get_mut(source.id.file()) {
				*slot = true;
			}
			files += 1;
			*languages.entry(source.language.as_str()).or_default() += 1;
		}
	}
	let mut symbols = 0usize;
	let mut navigable_symbols = 0usize;
	let mut non_navigable_symbols = 0usize;
	let mut kinds = BTreeMap::<&str, usize>::new();
	let mut shapes = BTreeMap::<&str, usize>::new();
	let mut symbol_counts = vec![0usize; selected_sources.len()];
	for symbol in snapshot.index.symbols.iter() {
		if !selected_sources
			.get(symbol.source.file())
			.copied()
			.unwrap_or(false)
			|| (!query.include_non_navigable && !symbol.navigable)
			|| (!query.kind.is_empty() && !query.kind.iter().any(|kind| kind == &symbol.kind))
			|| (!query.shape.is_empty()
				&& !query
					.shape
					.iter()
					.any(|shape| Shape::for_kind(symbol.kind.as_bytes()).as_str() == shape))
			|| name_filter
				.as_ref()
				.is_some_and(|regex| !regex.is_match(&symbol.name))
		{
			continue;
		}
		symbols += 1;
		if symbol.navigable {
			navigable_symbols += 1;
		} else {
			non_navigable_symbols += 1;
		}
		*kinds.entry(symbol.kind.as_str()).or_default() += 1;
		*shapes
			.entry(Shape::for_kind(symbol.kind.as_bytes()).as_str())
			.or_default() += 1;
		if let Some(count) = symbol_counts.get_mut(symbol.source.file()) {
			*count += 1;
		}
	}
	let mut references = 0usize;
	let mut ref_counts = vec![0usize; selected_sources.len()];
	for reference in snapshot.index.references.iter() {
		if selected_sources
			.get(reference.source.file())
			.copied()
			.unwrap_or(false)
		{
			references += 1;
			if let Some(count) = ref_counts.get_mut(reference.source.file()) {
				*count += 1;
			}
		}
	}
	let mut symbol_counts_by_path = BTreeMap::<&str, usize>::new();
	let mut ref_counts_by_path = BTreeMap::<&str, usize>::new();
	for source in &snapshot.index.sources {
		if let Some(count) = symbol_counts
			.get(source.id.file())
			.copied()
			.filter(|count| *count > 0)
		{
			*symbol_counts_by_path
				.entry(source.rel_path.as_str())
				.or_default() += count;
		}
		if let Some(count) = ref_counts
			.get(source.id.file())
			.copied()
			.filter(|count| *count > 0)
		{
			*ref_counts_by_path
				.entry(source.rel_path.as_str())
				.or_default() += count;
		}
	}
	let result = SymbolInsightsResult {
		files,
		symbols,
		references,
		navigable_symbols,
		non_navigable_symbols,
		languages: count_rows_borrowed(&languages),
		kinds: count_rows_borrowed(&kinds),
		shapes: count_rows_borrowed(&shapes),
		top_files_by_symbols: count_rows_borrowed(&symbol_counts_by_path),
		top_files_by_refs: count_rows_borrowed(&ref_counts_by_path),
	};
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolInsights(result),
		next_cursor: None,
	})
}

pub(crate) fn symbol_detail_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	workspace: Option<&str>,
	uri: &str,
	context_lines: usize,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, workspace)?;
	let symbol_scope = symbol_scope_for_roots(snapshot, roots, &selected_roots);
	let symbol = find_symbol(snapshot, &symbol_scope, uri)?;
	let source = WorkspaceView::new(snapshot)
		.sources()
		.record(&symbol.source)
		.ok_or_else(|| QueryError::new("source_not_found", "symbol source not found"))?;
	if source_root(roots, &selected_roots, source).is_none() {
		return Err(QueryError::new(
			"symbol_not_in_workspace",
			format!("symbol {uri} is not in the selected workspace"),
		));
	}
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolDetail(SymbolDetailResult {
			symbol: symbol_dto(symbol, source, roots),
			source: source_snippet(source, symbol, context_lines)?,
		}),
		next_cursor: None,
	})
}

pub(crate) fn symbol_usages_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SymbolUsagesQuery,
	page: Page,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let path_filter = FilePathFilter::compile(&query.path)
		.map_err(|err| QueryError::new("invalid_path_filter", err.to_string()))?;
	let symbol_scope = symbol_scope_for_roots(snapshot, roots, &selected_roots);
	let target = find_symbol(snapshot, &symbol_scope, &query.uri)?;
	let target_source = WorkspaceView::new(snapshot)
		.sources()
		.record(&target.source)
		.ok_or_else(|| QueryError::new("source_not_found", "target source not found"))?;
	if source_root(roots, &selected_roots, target_source).is_none() {
		return Err(QueryError::new(
			"symbol_not_in_workspace",
			format!("symbol {} is not in the selected workspace", query.uri),
		));
	}
	let mut incoming_rows = Vec::new();
	let mut outgoing_rows = Vec::new();
	let target_ordinals = query
		.include_descendants
		.then(|| snapshot.index.inventory.owner_and_descendants(&target.id));
	let targets = if let Some(target_ordinals) = &target_ordinals {
		target_ordinals
			.iter()
			.filter_map(|ordinal| snapshot.index.inventory.record(ordinal))
			.filter_map(|record| WorkspaceView::new(snapshot).symbols().find(&record.id))
			.collect::<Vec<_>>()
	} else {
		vec![target]
	};
	let usage_context = UsageDtoContext {
		snapshot,
		roots,
		selected_roots: &selected_roots,
		path_filter: &path_filter,
		langs: &query.lang,
	};
	if matches!(
		query.direction,
		UsageDirection::Incoming | UsageDirection::Both
	) {
		for selected in &targets {
			incoming_rows.extend(collect_incoming_usages(snapshot, selected, &usage_context));
		}
		if let Some(target_ordinals) = &target_ordinals {
			incoming_rows.retain(|row| !usage_source_is_in_set(snapshot, row, target_ordinals));
		}
		deduplicate_usage_rows(&mut incoming_rows);
	}
	if matches!(
		query.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	) {
		let references = WorkspaceView::new(snapshot).references();
		for selected in &targets {
			for id in references.outgoing_ids(&selected.id) {
				let Some(reference) = references.reference(&id) else {
					continue;
				};
				let internal = target_ordinals.as_ref().is_some_and(|target_ordinals| {
					resolved_reference_target(snapshot, &reference.id)
						.and_then(|id| snapshot.index.inventory.catalog().ordinal(&id))
						.is_some_and(|ordinal| target_ordinals.contains(ordinal))
				});
				if !internal
					&& let Some(row) =
						usage_dto(reference, UsageDirection::Outgoing, &usage_context)
				{
					outgoing_rows.push(row);
				}
			}
		}
		deduplicate_usage_rows(&mut outgoing_rows);
	}
	let incoming_summary = matches!(
		query.direction,
		UsageDirection::Incoming | UsageDirection::Both
	)
	.then(|| usage_summary(&incoming_rows, true));
	let outgoing_summary = matches!(
		query.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	)
	.then(|| usage_summary(&outgoing_rows, false));
	let mut rows = Vec::new();
	rows.extend(incoming_rows);
	rows.extend(outgoing_rows);
	rows.sort_by(usage_cmp_for_navigation);
	let page = expand_usage_page_to_group(&rows, page);
	let paged = page_rows(rows, page, current_generation)?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SymbolUsages(Box::new(SymbolUsagesResult {
			target: symbol_dto(target, target_source, roots),
			direction: query.direction,
			include_descendants: query.include_descendants,
			targets: targets.len(),
			total: paged.total,
			rows: paged.items,
			incoming_summary,
			outgoing_summary,
		})),
		next_cursor: paged.next_cursor,
	})
}

fn deduplicate_usage_rows(rows: &mut Vec<UsageDto>) {
	let mut seen = BTreeSet::new();
	rows.retain(|row| seen.insert(row.reference.clone()));
}

fn usage_source_is_in_set(
	snapshot: &WorkspaceSnapshot,
	usage: &UsageDto,
	targets: &SymbolSet,
) -> bool {
	ReferenceId::parse(&usage.reference)
		.and_then(|id| WorkspaceView::new(snapshot).references().reference(&id))
		.and_then(|reference| {
			snapshot
				.index
				.inventory
				.catalog()
				.ordinal(&reference.source_symbol)
		})
		.is_some_and(|ordinal| targets.contains(ordinal))
}

pub(crate) fn view_read_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: ViewReadQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	let result = views::read(
		&query.uri,
		roots,
		query.scheme.as_deref().unwrap_or(DEFAULT_SCHEME),
		snapshot,
		query.context_lines,
		query.include_code,
	)
	.map_err(|err| QueryError::new("view_read_failed", err.to_string()))?;
	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::ViewRead(result),
		next_cursor: None,
	})
}

fn collect_incoming_usages(
	snapshot: &WorkspaceSnapshot,
	target: &SymbolRecord,
	context: &UsageDtoContext<'_>,
) -> Vec<UsageDto> {
	let references = WorkspaceView::new(snapshot).references();
	let mut rows = references
		.incoming_ids(&target.id)
		.into_iter()
		.filter_map(|id| references.reference(&id))
		.filter_map(|reference| usage_dto(reference, UsageDirection::Incoming, context))
		.collect::<Vec<_>>();
	let mut seen = rows
		.iter()
		.filter_map(|row| ReferenceId::parse(&row.reference))
		.collect::<BTreeSet<_>>();
	let mut visited = BTreeSet::from([target.id]);
	collect_indirect_incoming_usages(
		snapshot,
		&target.id,
		context,
		IndirectUsageState {
			depth: 0,
			visited: &mut visited,
			seen: &mut seen,
			rows: &mut rows,
		},
	);
	rows
}

struct IndirectUsageState<'a> {
	depth: usize,
	visited: &'a mut BTreeSet<SymbolId>,
	seen: &'a mut BTreeSet<ReferenceId>,
	rows: &'a mut Vec<UsageDto>,
}

fn collect_indirect_incoming_usages(
	snapshot: &WorkspaceSnapshot,
	target: &SymbolId,
	context: &UsageDtoContext<'_>,
	state: IndirectUsageState<'_>,
) {
	const MAX_INDIRECT_USAGE_DEPTH: usize = 4;
	if state.depth >= MAX_INDIRECT_USAGE_DEPTH {
		return;
	}
	let references = WorkspaceView::new(snapshot).references();
	let symbols = WorkspaceView::new(snapshot).symbols();
	let aliases = references
		.incoming_ids(target)
		.into_iter()
		.filter_map(|id| references.reference(&id))
		.filter(|reference| reference.kind == "uses_type")
		.filter_map(|reference| symbols.find(&reference.source_symbol))
		.filter(|symbol| symbol.kind == "type")
		.filter(|symbol| state.visited.insert(symbol.id))
		.collect::<Vec<_>>();
	for alias in aliases {
		collect_direct_usages_via(snapshot, alias, context, state.seen, state.rows);
		collect_indirect_incoming_usages(
			snapshot,
			&alias.id,
			context,
			IndirectUsageState {
				depth: state.depth + 1,
				visited: state.visited,
				seen: state.seen,
				rows: state.rows,
			},
		);
	}
}

fn collect_direct_usages_via(
	snapshot: &WorkspaceSnapshot,
	alias: &SymbolRecord,
	context: &UsageDtoContext<'_>,
	seen: &mut BTreeSet<ReferenceId>,
	rows: &mut Vec<UsageDto>,
) {
	let references = WorkspaceView::new(snapshot).references();
	for id in references.incoming_ids(&alias.id) {
		let Some(reference) = references.reference(&id) else {
			continue;
		};
		if reference.source_symbol == alias.id || !seen.insert(reference.id) {
			continue;
		}
		let Some(mut row) = usage_dto(reference, UsageDirection::Incoming, context) else {
			continue;
		};
		row.via = Some(format!("{} ({})", alias.name, alias.identity));
		rows.push(row);
	}
}

fn usage_cmp_for_navigation(left: &UsageDto, right: &UsageDto) -> std::cmp::Ordering {
	usage_direction_priority(left.direction)
		.cmp(&usage_direction_priority(right.direction))
		.then_with(|| usage_kind_priority(&left.kind).cmp(&usage_kind_priority(&right.kind)))
		.then_with(|| left.root.cmp(&right.root))
		.then_with(|| left.file.cmp(&right.file))
		.then_with(|| left.actor.cmp(&right.actor))
		.then_with(|| left.context.cmp(&right.context))
		.then_with(|| left.endpoint.cmp(&right.endpoint))
		.then_with(|| left.via.cmp(&right.via))
		.then_with(|| left.line_range.cmp(&right.line_range))
		.then_with(|| left.reference.cmp(&right.reference))
}

fn expand_usage_page_to_group(rows: &[UsageDto], mut page: Page) -> Page {
	let start = page
		.cursor
		.as_ref()
		.map(|cursor| cursor.offset)
		.unwrap_or(0);
	if page.limit == 0 || start >= rows.len() {
		return page;
	}
	let mut end = start.saturating_add(page.limit).min(rows.len());
	while end < rows.len() && same_usage_group(&rows[end - 1], &rows[end]) {
		end += 1;
	}
	page.limit = end - start;
	page
}

fn same_usage_group(left: &UsageDto, right: &UsageDto) -> bool {
	left.direction == right.direction
		&& left.kind == right.kind
		&& left.root == right.root
		&& left.file == right.file
		&& left.via == right.via
		&& match left.direction {
			UsageDirection::Incoming => left.actor == right.actor && left.context == right.context,
			UsageDirection::Outgoing => left.endpoint == right.endpoint,
			UsageDirection::Both => {
				left.actor == right.actor
					&& left.context == right.context
					&& left.endpoint == right.endpoint
			}
		}
}

fn usage_direction_priority(direction: UsageDirection) -> u8 {
	match direction {
		UsageDirection::Incoming => 0,
		UsageDirection::Outgoing => 1,
		UsageDirection::Both => 2,
	}
}

fn usage_kind_priority(kind: &str) -> u8 {
	match kind {
		"calls" | "constructs" => 10,
		"extends" | "implements" | "inherits" => 20,
		"reads" | "uses_type" | "returns_type" | "annotates" => 30,
		"imports" => 40,
		_ => 50,
	}
}

fn usage_summary(rows: &[UsageDto], shared_signal: bool) -> UsageSummaryDto {
	let mut files = BTreeSet::new();
	let mut contexts = BTreeSet::new();
	let mut prefixes = BTreeMap::<&str, usize>::new();
	let mut kinds = BTreeMap::<&str, usize>::new();
	let mut actors = BTreeMap::<&str, usize>::new();
	for row in rows {
		files.insert(row.file.as_str());
		contexts.insert(row.context.as_str());
		*prefixes.entry(row.prefix.as_str()).or_default() += 1;
		*kinds.entry(row.kind.as_str()).or_default() += 1;
		*actors.entry(row.actor.as_str()).or_default() += 1;
	}
	let top_prefixes = count_rows_borrowed(&prefixes);
	let dominant_prefix = top_prefixes
		.first()
		.map(|row| {
			format!(
				"{} ({} refs, {}%)",
				row.name,
				row.count,
				percent(row.count, rows.len())
			)
		})
		.unwrap_or_default();
	UsageSummaryDto {
		refs: rows.len(),
		files: files.len(),
		contexts: contexts.len(),
		prefixes: prefixes.len(),
		dominant_prefix,
		kinds: count_rows_borrowed(&kinds),
		top_actors: count_rows_borrowed(&actors),
		top_prefixes,
		shared_helper_signal: if shared_signal {
			shared_helper_signal(rows.len(), files.len(), contexts.len(), prefixes)
		} else {
			String::new()
		},
	}
}

fn shared_helper_signal(
	refs: usize,
	files: usize,
	contexts: usize,
	prefixes: BTreeMap<&str, usize>,
) -> String {
	if refs == 0 {
		return "unused_or_unresolved".to_string();
	}
	let prefix_count = prefixes.len();
	let dominant = count_rows_borrowed(&prefixes)
		.first()
		.map(|row| percent(row.count, refs))
		.unwrap_or(0);
	if files >= 3 && contexts >= 3 && prefix_count >= 2 {
		"shared_helper_candidate".to_string()
	} else if files <= 1 || dominant >= 80 {
		"localized_not_shared".to_string()
	} else {
		"mixed_review_needed".to_string()
	}
}

fn count_rows_borrowed(counts: &BTreeMap<&str, usize>) -> Vec<CountDto> {
	let mut rows = counts
		.iter()
		.map(|(name, count)| CountDto {
			name: (*name).to_string(),
			count: *count,
		})
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
	rows
}

fn percent(count: usize, total: usize) -> usize {
	count
		.checked_mul(100)
		.and_then(|value| value.checked_div(total))
		.unwrap_or(0)
}
