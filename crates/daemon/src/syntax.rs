use std::path::PathBuf;

use code_moniker_core::lang::Lang;
use code_moniker_query::{
	QueryError, QueryResponse, QueryResult, SYNTAX_TREE_MAX_DEPTH, SYNTAX_TREE_MAX_NODES,
	SYNTAX_TREE_MAX_TEXT_CHARS, SyntaxNodeDto, SyntaxPointDto, SyntaxTreeQuery, SyntaxTreeResult,
	WorkspaceGeneration,
};
use code_moniker_workspace::snapshot::{
	SourceFileRecord, SymbolRecord, WorkspaceSnapshot, WorkspaceView,
};

use super::{find_symbol, load_source_text, selected_roots, source_root};

pub(super) fn syntax_tree_response(
	snapshot: &WorkspaceSnapshot,
	roots: &[PathBuf],
	query: SyntaxTreeQuery,
	current_generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	validate_limits(&query)?;
	let selected_roots = selected_roots(roots, query.workspace.as_deref())?;
	let (source, symbol) = resolve_focus(snapshot, roots, &selected_roots, &query.focus)?;
	let source_text = load_source_text(source)?;
	let lang = Lang::from_tag(&source.language).ok_or_else(|| {
		QueryError::new(
			"syntax_language_unsupported",
			format!(
				"source {} has unsupported language tag `{}`",
				source.rel_path, source.language
			),
		)
	})?;
	let tree = lang.parse(&source.rel_path, &source_text);
	let focus_line_range = symbol.and_then(|symbol| symbol.line_range);
	let root_node = symbol
		.and_then(|symbol| {
			symbol
				.line_range
				.and_then(|range| find_symbol_node(tree.root_node(), range, symbol, &source_text))
		})
		.or_else(|| focus_line_range.and_then(|range| find_focus_node(tree.root_node(), range)))
		.unwrap_or_else(|| tree.root_node());
	let total_nodes = count_nodes(root_node, query.named_only);
	let mut emitted_nodes = 0;
	let root =
		build_node(root_node, &source_text, &query, 0, &mut emitted_nodes).ok_or_else(|| {
			QueryError::new("syntax_tree_empty", "syntax tree has no renderable root")
		})?;

	Ok(QueryResponse {
		generation: current_generation,
		result: QueryResult::SyntaxTree(SyntaxTreeResult {
			file: source.rel_path.clone(),
			language: source.language.clone(),
			focus: query.focus,
			focus_line_range,
			root,
			emitted_nodes,
			total_nodes,
			max_depth: query.max_depth,
			truncated: emitted_nodes < total_nodes,
			has_error: root_node.has_error(),
		}),
		next_cursor: None,
	})
}

fn validate_limits(query: &SyntaxTreeQuery) -> Result<(), QueryError> {
	if query.max_depth > SYNTAX_TREE_MAX_DEPTH {
		return Err(QueryError::new(
			"invalid_syntax_depth",
			format!("max_depth must be <= {SYNTAX_TREE_MAX_DEPTH}"),
		));
	}
	if query.max_nodes == 0 || query.max_nodes > SYNTAX_TREE_MAX_NODES {
		return Err(QueryError::new(
			"invalid_syntax_node_limit",
			format!("max_nodes must be between 1 and {SYNTAX_TREE_MAX_NODES}"),
		));
	}
	if query.max_text_chars > SYNTAX_TREE_MAX_TEXT_CHARS {
		return Err(QueryError::new(
			"invalid_syntax_text_limit",
			format!("max_text_chars must be <= {SYNTAX_TREE_MAX_TEXT_CHARS}"),
		));
	}
	Ok(())
}

fn resolve_focus<'a>(
	snapshot: &'a WorkspaceSnapshot,
	roots: &[PathBuf],
	selected_roots: &[&PathBuf],
	focus: &str,
) -> Result<(&'a SourceFileRecord, Option<&'a SymbolRecord>), QueryError> {
	if is_symbol_focus(focus) {
		let symbol = find_symbol(snapshot, focus)?;
		let source = WorkspaceView::new(snapshot)
			.sources()
			.record(&symbol.source)
			.ok_or_else(|| QueryError::new("source_not_found", "symbol source not found"))?;
		if source_root(roots, selected_roots, source).is_none() {
			return Err(QueryError::new(
				"symbol_not_in_workspace",
				format!("symbol {focus} is not in the selected workspace"),
			));
		}
		return Ok((source, Some(symbol)));
	}

	let normalized = focus.strip_prefix("./").unwrap_or(focus);
	let mut matches = snapshot
		.index
		.sources
		.iter()
		.filter(|source| source_root(roots, selected_roots, source).is_some())
		.filter(|source| source.rel_path == normalized || source.path == focus);
	let source = matches.next().ok_or_else(|| {
		QueryError::new(
			"source_not_found",
			format!("source `{focus}` was not found in the selected workspace"),
		)
	})?;
	if matches.next().is_some() {
		return Err(QueryError::new(
			"source_ambiguous",
			format!("source `{focus}` exists in more than one selected workspace root"),
		));
	}
	Ok((source, None))
}

fn is_symbol_focus(focus: &str) -> bool {
	focus.starts_with("code+moniker://")
		|| focus.starts_with("symbol:")
		|| focus
			.split_once(':')
			.is_some_and(|(tag, rest)| Lang::from_tag(tag).is_some() && rest.contains('/'))
}

fn find_focus_node(
	node: tree_sitter::Node<'_>,
	line_range: (u32, u32),
) -> Option<tree_sitter::Node<'_>> {
	if !node_contains_line_range(node, line_range) {
		return None;
	}
	let children = children(node, true);
	if let Some(exact) = children
		.iter()
		.find(|child| node_line_range(**child) == line_range)
	{
		return Some(*exact);
	}
	for child in children {
		if let Some(found) = find_focus_node(child, line_range) {
			return Some(found);
		}
	}
	Some(node)
}

fn find_symbol_node<'tree>(
	node: tree_sitter::Node<'tree>,
	line_range: (u32, u32),
	symbol: &SymbolRecord,
	source: &str,
) -> Option<tree_sitter::Node<'tree>> {
	find_best_symbol_node(node, line_range, symbol, source, true)
		.or_else(|| find_best_symbol_node(node, line_range, symbol, source, false))
}

fn find_best_symbol_node<'tree>(
	node: tree_sitter::Node<'tree>,
	line_range: (u32, u32),
	symbol: &SymbolRecord,
	source: &str,
	require_kind_match: bool,
) -> Option<tree_sitter::Node<'tree>> {
	if !node_contains_line_range(node, line_range) {
		return None;
	}
	let mut best = None;
	for child in children(node, true) {
		if let Some(found) =
			find_best_symbol_node(child, line_range, symbol, source, require_kind_match)
		{
			best = choose_more_specific(best, found, line_range);
		}
	}
	let candidate = node
		.child_by_field_name("name")
		.and_then(|name_node| name_node.utf8_text(source.as_bytes()).ok())
		.filter(|parsed_name| {
			let symbol_name = symbol
				.call_name
				.as_deref()
				.unwrap_or_else(|| symbol.name.split('(').next().unwrap_or(&symbol.name));
			names_match(parsed_name, symbol_name)
				&& (!require_kind_match || node_kind_matches_symbol(node.kind(), &symbol.kind))
		})
		.map(|_| node);
	candidate
		.map(|candidate| choose_more_specific(best, candidate, line_range))
		.unwrap_or(best)
}

fn choose_more_specific<'tree>(
	current: Option<tree_sitter::Node<'tree>>,
	candidate: tree_sitter::Node<'tree>,
	line_range: (u32, u32),
) -> Option<tree_sitter::Node<'tree>> {
	let Some(current) = current else {
		return Some(candidate);
	};
	let candidate_exact = node_line_range(candidate) == line_range;
	let current_exact = node_line_range(current) == line_range;
	if candidate_exact != current_exact {
		return Some(if candidate_exact { candidate } else { current });
	}
	let candidate_span = candidate.end_byte().saturating_sub(candidate.start_byte());
	let current_span = current.end_byte().saturating_sub(current.start_byte());
	Some(if candidate_span < current_span {
		candidate
	} else {
		current
	})
}

fn node_kind_matches_symbol(node_kind: &str, symbol_kind: &str) -> bool {
	node_kind == symbol_kind
		|| ["_declaration", "_definition", "_item", "_statement"]
			.iter()
			.any(|suffix| node_kind.strip_suffix(suffix) == Some(symbol_kind))
		|| matches!(
			(symbol_kind, node_kind),
			("fn", "function_item")
				| ("fn", "function_definition")
				| ("fn", "function_declaration")
				| ("type", "type_item")
				| ("type", "type_alias_declaration")
				| ("interface", "interface_declaration")
				| ("trait", "trait_item")
		)
}

fn names_match(parsed: &str, symbol: &str) -> bool {
	let parsed = parsed.trim_matches(['"', '`']);
	let symbol = symbol.trim_matches(['"', '`']);
	parsed == symbol
		|| parsed
			.rsplit(['.', ':'])
			.next()
			.is_some_and(|tail| tail == symbol)
}

fn node_contains_line_range(node: tree_sitter::Node<'_>, range: (u32, u32)) -> bool {
	let node_range = node_line_range(node);
	node_range.0 <= range.0 && node_range.1 >= range.1
}

fn node_line_range(node: tree_sitter::Node<'_>) -> (u32, u32) {
	(
		saturating_u32(node.start_position().row).saturating_add(1),
		saturating_u32(node.end_position().row).saturating_add(1),
	)
}

fn count_nodes(node: tree_sitter::Node<'_>, named_only: bool) -> usize {
	let mut count = 0usize;
	let mut pending = vec![node];
	while let Some(node) = pending.pop() {
		count = count.saturating_add(1);
		pending.extend(children(node, named_only));
	}
	count
}

fn build_node(
	node: tree_sitter::Node<'_>,
	source: &str,
	query: &SyntaxTreeQuery,
	depth: usize,
	emitted: &mut usize,
) -> Option<SyntaxNodeDto> {
	if *emitted >= query.max_nodes {
		return None;
	}
	*emitted += 1;
	let eligible_children = children(node, query.named_only);
	let text = (query.include_text && eligible_children.is_empty())
		.then(|| bounded_text(node, source, query.max_text_chars))
		.flatten();
	let mut rendered_children = Vec::new();
	if depth < query.max_depth {
		for child in eligible_children {
			let Some(child) = build_node(child, source, query, depth + 1, emitted) else {
				break;
			};
			rendered_children.push(child);
		}
	}
	let start = node.start_position();
	let end = node.end_position();
	Some(SyntaxNodeDto {
		kind: node.kind().to_string(),
		named: node.is_named(),
		error: node.is_error(),
		missing: node.is_missing(),
		byte_range: (node.start_byte(), node.end_byte()),
		start: SyntaxPointDto {
			line: saturating_u32(start.row).saturating_add(1),
			column: saturating_u32(start.column),
		},
		end: SyntaxPointDto {
			line: saturating_u32(end.row).saturating_add(1),
			column: saturating_u32(end.column),
		},
		text,
		children: rendered_children,
	})
}

fn children(node: tree_sitter::Node<'_>, named_only: bool) -> Vec<tree_sitter::Node<'_>> {
	let mut cursor = node.walk();
	if named_only {
		node.named_children(&mut cursor).collect()
	} else {
		node.children(&mut cursor).collect()
	}
}

fn bounded_text(node: tree_sitter::Node<'_>, source: &str, max_chars: usize) -> Option<String> {
	if max_chars == 0 {
		return None;
	}
	let text = node.utf8_text(source.as_bytes()).ok()?;
	let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
	if normalized.is_empty() {
		return None;
	}
	let mut bounded = normalized.chars().take(max_chars).collect::<String>();
	if normalized.chars().count() > max_chars {
		bounded.push('…');
	}
	Some(bounded)
}

fn saturating_u32(value: usize) -> u32 {
	u32::try_from(value).unwrap_or(u32::MAX)
}
