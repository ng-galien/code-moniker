use std::path::PathBuf;

use code_moniker_core::lang::{Lang, ParsedDocument, SyntaxInjection, parse_source};
use code_moniker_query::{
	QueryError, QueryResponse, QueryResult, SYNTAX_PARSE_MAX_SOURCE_BYTES, SYNTAX_TREE_MAX_DEPTH,
	SYNTAX_TREE_MAX_NODES, SYNTAX_TREE_MAX_TEXT_CHARS, SyntaxNodeDto, SyntaxParseQuery,
	SyntaxPointDto, SyntaxTreeQuery, SyntaxTreeResult, WorkspaceGeneration,
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
	let options = SyntaxRenderOptions::from(&query);
	validate_limits(options)?;
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
	let document = lang.parse(&source.rel_path, &source_text);
	let primary = document.primary();
	let focus_line_range = symbol.and_then(|symbol| symbol.line_range);
	let root_node = symbol
		.and_then(|symbol| {
			symbol.line_range.and_then(|range| {
				find_symbol_node(primary.root_node(), range, symbol, &source_text)
			})
		})
		.or_else(|| focus_line_range.and_then(|range| find_focus_node(primary.root_node(), range)))
		.unwrap_or_else(|| primary.root_node());
	render_document(
		&document,
		root_node,
		&source_text,
		SyntaxResponseMetadata {
			file: source.rel_path.clone(),
			language: source.language.clone(),
			focus: query.focus,
			focus_line_range,
			generation: current_generation,
		},
		options,
	)
}

pub(super) fn syntax_parse_response(query: SyntaxParseQuery) -> Result<QueryResponse, QueryError> {
	let options = SyntaxRenderOptions::from(&query);
	validate_limits(options)?;
	if query.source.len() > SYNTAX_PARSE_MAX_SOURCE_BYTES {
		return Err(QueryError::new(
			"syntax_source_too_large",
			format!("source must be at most {SYNTAX_PARSE_MAX_SOURCE_BYTES} bytes"),
		));
	}
	let uri = query
		.uri
		.unwrap_or_else(|| default_source_uri(&query.language));
	let document = parse_source(&query.language, &uri, &query.source).ok_or_else(|| {
		QueryError::new(
			"syntax_language_unsupported",
			format!("unsupported language tag `{}`", query.language),
		)
	})?;
	let root_node = document.primary().root_node();
	render_document(
		&document,
		root_node,
		&query.source,
		SyntaxResponseMetadata {
			file: uri.clone(),
			language: query.language,
			focus: uri,
			focus_line_range: None,
			generation: None,
		},
		options,
	)
}

#[derive(Clone, Copy)]
struct SyntaxRenderOptions {
	max_depth: usize,
	max_nodes: usize,
	named_only: bool,
	include_text: bool,
	max_text_chars: usize,
}

impl From<&SyntaxTreeQuery> for SyntaxRenderOptions {
	fn from(query: &SyntaxTreeQuery) -> Self {
		Self {
			max_depth: query.max_depth,
			max_nodes: query.max_nodes,
			named_only: query.named_only,
			include_text: query.include_text,
			max_text_chars: query.max_text_chars,
		}
	}
}

impl From<&SyntaxParseQuery> for SyntaxRenderOptions {
	fn from(query: &SyntaxParseQuery) -> Self {
		Self {
			max_depth: query.max_depth,
			max_nodes: query.max_nodes,
			named_only: query.named_only,
			include_text: query.include_text,
			max_text_chars: query.max_text_chars,
		}
	}
}

struct SyntaxResponseMetadata {
	file: String,
	language: String,
	focus: String,
	focus_line_range: Option<(u32, u32)>,
	generation: Option<WorkspaceGeneration>,
}

fn render_document(
	document: &ParsedDocument,
	root_node: tree_sitter::Node<'_>,
	source: &str,
	metadata: SyntaxResponseMetadata,
	options: SyntaxRenderOptions,
) -> Result<QueryResponse, QueryError> {
	let total_nodes = count_document_nodes(document, root_node, options.named_only);
	let mut emitted_nodes = 0;
	let root = build_document_node(document, root_node, source, options, 0, &mut emitted_nodes)
		.ok_or_else(|| {
			QueryError::new("syntax_tree_empty", "syntax tree has no renderable root")
		})?;
	let has_error = root_node.has_error()
		|| document
			.injections()
			.iter()
			.filter(|injection| {
				let range = injection.host_byte_range();
				range.start >= root_node.start_byte() && range.end <= root_node.end_byte()
			})
			.any(|injection| injection.tree().root_node().has_error());

	Ok(QueryResponse {
		generation: metadata.generation,
		result: QueryResult::SyntaxTree(SyntaxTreeResult {
			file: metadata.file,
			language: metadata.language,
			focus: metadata.focus,
			focus_line_range: metadata.focus_line_range,
			root,
			emitted_nodes,
			total_nodes,
			max_depth: options.max_depth,
			truncated: emitted_nodes < total_nodes,
			has_error,
		}),
		next_cursor: None,
	})
}

fn validate_limits(options: SyntaxRenderOptions) -> Result<(), QueryError> {
	if options.max_depth > SYNTAX_TREE_MAX_DEPTH {
		return Err(QueryError::new(
			"invalid_syntax_depth",
			format!("max_depth must be <= {SYNTAX_TREE_MAX_DEPTH}"),
		));
	}
	if options.max_nodes == 0 || options.max_nodes > SYNTAX_TREE_MAX_NODES {
		return Err(QueryError::new(
			"invalid_syntax_node_limit",
			format!("max_nodes must be between 1 and {SYNTAX_TREE_MAX_NODES}"),
		));
	}
	if options.max_text_chars > SYNTAX_TREE_MAX_TEXT_CHARS {
		return Err(QueryError::new(
			"invalid_syntax_text_limit",
			format!("max_text_chars must be <= {SYNTAX_TREE_MAX_TEXT_CHARS}"),
		));
	}
	Ok(())
}

fn default_source_uri(language: &str) -> String {
	let extension = match language {
		"rs" => "rs",
		"ts" => "ts",
		"java" => "java",
		"python" => "py",
		"go" => "go",
		"c" => "c",
		"cs" => "cs",
		"sql" => "sql",
		"plpgsql" => "plpgsql",
		other => other,
	};
	format!("snippet.{extension}")
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
			.is_some_and(|(tag, _)| Lang::from_tag(tag).is_some())
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

fn count_document_nodes(
	document: &ParsedDocument,
	node: tree_sitter::Node<'_>,
	named_only: bool,
) -> usize {
	let mut count = 0usize;
	let mut pending = vec![node];
	while let Some(node) = pending.pop() {
		count = count.saturating_add(1);
		if node.kind() == "dollar_quoted_string"
			&& let Some(injection) = document.injection_for_host(node.start_byte()..node.end_byte())
		{
			count = count.saturating_add(count_nodes(injection.tree().root_node(), named_only));
		}
		pending.extend(children(node, named_only));
	}
	count
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

fn build_document_node(
	document: &ParsedDocument,
	node: tree_sitter::Node<'_>,
	source: &str,
	options: SyntaxRenderOptions,
	depth: usize,
	emitted: &mut usize,
) -> Option<SyntaxNodeDto> {
	let injection = (node.kind() == "dollar_quoted_string")
		.then(|| document.injection_for_host(node.start_byte()..node.end_byte()))
		.flatten();
	if *emitted >= options.max_nodes {
		return None;
	}
	*emitted += 1;
	let eligible_children = children(node, options.named_only);
	let text = (options.include_text && eligible_children.is_empty() && injection.is_none())
		.then(|| bounded_text(node, source, options.max_text_chars))
		.flatten();
	let mut rendered_children = Vec::new();
	if depth < options.max_depth {
		if let Some(injection) = injection
			&& let Some(child) =
				build_injection_node(injection, source, options, depth + 1, emitted)
		{
			rendered_children.push(child);
		}
		for child in eligible_children {
			let Some(child) =
				build_document_node(document, child, source, options, depth + 1, emitted)
			else {
				break;
			};
			rendered_children.push(child);
		}
	}
	let start = node.start_position();
	let end = node.end_position();
	Some(SyntaxNodeDto {
		kind: node.kind().to_string(),
		language: None,
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

fn build_injection_node(
	injection: &SyntaxInjection,
	host_source: &str,
	options: SyntaxRenderOptions,
	depth: usize,
	emitted: &mut usize,
) -> Option<SyntaxNodeDto> {
	let content_range = injection.content_byte_range();
	let content = host_source.get(content_range.clone())?;
	let origin = source_origin(host_source, content_range.start, injection.language());
	build_injected_tree_node(
		injection.tree().root_node(),
		content,
		options,
		depth,
		emitted,
		origin,
	)
}

#[derive(Clone, Copy)]
struct SourceOrigin {
	byte: usize,
	row: usize,
	column: usize,
	language: &'static str,
}

fn source_origin(source: &str, byte: usize, language: &'static str) -> SourceOrigin {
	let prefix = source.get(..byte).unwrap_or_default();
	let row = prefix.bytes().filter(|byte| *byte == b'\n').count();
	let column = prefix
		.rsplit_once('\n')
		.map(|(_, tail)| tail.len())
		.unwrap_or(prefix.len());
	SourceOrigin {
		byte,
		row,
		column,
		language,
	}
}

fn build_injected_tree_node(
	node: tree_sitter::Node<'_>,
	source: &str,
	options: SyntaxRenderOptions,
	depth: usize,
	emitted: &mut usize,
	origin: SourceOrigin,
) -> Option<SyntaxNodeDto> {
	if *emitted >= options.max_nodes {
		return None;
	}
	*emitted += 1;
	let eligible_children = children(node, options.named_only);
	let text = (options.include_text && eligible_children.is_empty())
		.then(|| bounded_text(node, source, options.max_text_chars))
		.flatten();
	let mut rendered_children = Vec::new();
	if depth < options.max_depth {
		for child in eligible_children {
			let Some(child) =
				build_injected_tree_node(child, source, options, depth + 1, emitted, origin)
			else {
				break;
			};
			rendered_children.push(child);
		}
	}
	let start = translated_point(node.start_position(), origin);
	let end = translated_point(node.end_position(), origin);
	Some(SyntaxNodeDto {
		kind: node.kind().to_string(),
		language: node.parent().is_none().then(|| origin.language.to_string()),
		named: node.is_named(),
		error: node.is_error(),
		missing: node.is_missing(),
		byte_range: (
			origin.byte.saturating_add(node.start_byte()),
			origin.byte.saturating_add(node.end_byte()),
		),
		start,
		end,
		text,
		children: rendered_children,
	})
}

fn translated_point(point: tree_sitter::Point, origin: SourceOrigin) -> SyntaxPointDto {
	SyntaxPointDto {
		line: saturating_u32(origin.row.saturating_add(point.row)).saturating_add(1),
		column: saturating_u32(if point.row == 0 {
			origin.column.saturating_add(point.column)
		} else {
			point.column
		}),
	}
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
