use std::path::PathBuf;

use code_moniker_core::lang::{
	Lang, ParsedDocument, SyntaxEntryPoint, SyntaxInjection, parse_source,
};
use code_moniker_query::{
	QueryError, QueryResponse, QueryResult, SYNTAX_PARSE_MAX_SOURCE_BYTES,
	SYNTAX_TREE_MAX_TEXT_CHARS, SyntaxNodeDto, SyntaxParseQuery, SyntaxPointDto, SyntaxTreeQuery,
	SyntaxTreeResult, WorkspaceGeneration,
};
use code_moniker_workspace::snapshot::{
	SourceFileRecord, SymbolRecord, WorkspaceSnapshot, WorkspaceView,
};

use crate::helpers::{
	find_symbol, load_source_text, selected_roots, source_root, symbol_scope_for_roots,
};

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
	// Only routine bodies fold their errors into the document's: an embedded statement or
	// expression region carries PL/pgSQL-only clauses (`INTO STRICT v`) the SQL grammar cannot
	// read — plpgsql itself strips them before parsing — so its errors stay region-local facts.
	let has_error = root_node.has_error()
		|| document
			.injections()
			.iter()
			.filter(|injection| {
				matches!(
					injection.entry_point(),
					SyntaxEntryPoint::Block | SyntaxEntryPoint::Script
				)
			})
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
	if options.max_nodes == 0 {
		return Err(QueryError::new(
			"invalid_syntax_node_limit",
			"max_nodes must be greater than 0",
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
		let symbol_scope = symbol_scope_for_roots(snapshot, roots, selected_roots);
		let symbol = find_symbol(snapshot, &symbol_scope, focus)?;
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

/// The nodes an injected host node stands in for: `dollar_quoted_string` carries a routine body,
/// `sql_expression` a SQL region the PL/pgSQL grammar keeps opaque.
fn document_injection<'a>(
	document: &'a ParsedDocument,
	node: tree_sitter::Node<'_>,
) -> Option<&'a SyntaxInjection> {
	matches!(node.kind(), "dollar_quoted_string" | "sql_expression")
		.then(|| document.injection_for_host(node.start_byte()..node.end_byte()))
		.flatten()
}

fn injection_total_nodes(injection: &SyntaxInjection, named_only: bool) -> usize {
	injection.nested().iter().fold(
		count_nodes(injection.render_root(), named_only),
		|total, nested| total.saturating_add(injection_total_nodes(nested, named_only)),
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
		if let Some(injection) = document_injection(document, node) {
			count = count.saturating_add(injection_total_nodes(injection, named_only));
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
	let injection = document_injection(document, node);
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
		entry_point: None,
		has_error: None,
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
	let origin = SourceOrigin {
		prefix: injection.analysis_prefix(),
		..source_origin(host_source, content_range.start, injection.language())
	};
	let analysis = injection.analysis_source().map(str::to_string);
	build_injected_tree_node(
		injection,
		injection.render_root(),
		analysis.as_deref().unwrap_or(content),
		options,
		depth,
		emitted,
		origin,
		true,
	)
}

#[derive(Clone, Copy)]
struct SourceOrigin {
	byte: usize,
	row: usize,
	column: usize,
	language: &'static str,
	/// Byte length of the injection's synthetic analysis prefix ("SELECT "); never spans a line.
	prefix: usize,
}

fn source_origin(source: &str, byte: usize, language: &'static str) -> SourceOrigin {
	let leading = source.get(..byte).unwrap_or_default();
	let row = leading.bytes().filter(|byte| *byte == b'\n').count();
	let column = leading
		.rsplit_once('\n')
		.map(|(_, tail)| tail.len())
		.unwrap_or(leading.len());
	SourceOrigin {
		byte,
		row,
		column,
		language,
		prefix: 0,
	}
}

#[allow(clippy::too_many_arguments)]
fn build_injected_tree_node(
	injection: &SyntaxInjection,
	node: tree_sitter::Node<'_>,
	source: &str,
	options: SyntaxRenderOptions,
	depth: usize,
	emitted: &mut usize,
	origin: SourceOrigin,
	root: bool,
) -> Option<SyntaxNodeDto> {
	let document_start = origin
		.byte
		.saturating_add(node.start_byte().saturating_sub(origin.prefix));
	let document_end = origin
		.byte
		.saturating_add(node.end_byte().saturating_sub(origin.prefix));
	let nested = (node.kind() == "sql_expression")
		.then(|| injection.nested_for_host(document_start..document_end))
		.flatten();
	if *emitted >= options.max_nodes {
		return None;
	}
	*emitted += 1;
	let eligible_children = children(node, options.named_only);
	let text = (options.include_text && eligible_children.is_empty() && nested.is_none())
		.then(|| bounded_text(node, source, options.max_text_chars))
		.flatten();
	let mut rendered_children = Vec::new();
	if depth < options.max_depth {
		if let Some(nested) = nested {
			let content_range = nested.content_byte_range();
			let local_range = content_range.start.saturating_sub(origin.byte)
				..content_range.end.saturating_sub(origin.byte);
			if source.get(local_range.clone()).is_some() {
				// Positions inside `source` are relative to the parent injection's content;
				// chain them onto the parent origin so the nested origin speaks document
				// coordinates like every other.
				let local = source_origin(source, local_range.start, nested.language());
				let nested_origin = SourceOrigin {
					byte: content_range.start,
					row: origin.row.saturating_add(local.row),
					column: if local.row == 0 {
						origin.column.saturating_add(local.column)
					} else {
						local.column
					},
					language: nested.language(),
					prefix: nested.analysis_prefix(),
				};
				let content = &source[local_range];
				let analysis = nested.analysis_source().map(str::to_string);
				if let Some(child) = build_injected_tree_node(
					nested,
					nested.render_root(),
					analysis.as_deref().unwrap_or(content),
					options,
					depth + 1,
					emitted,
					nested_origin,
					true,
				) {
					rendered_children.push(child);
				}
			}
		}
		for child in eligible_children {
			let Some(child) = build_injected_tree_node(
				injection,
				child,
				source,
				options,
				depth + 1,
				emitted,
				origin,
				false,
			) else {
				break;
			};
			rendered_children.push(child);
		}
	}
	let start = translated_point(node.start_position(), origin);
	let end = translated_point(node.end_position(), origin);
	Some(SyntaxNodeDto {
		kind: node.kind().to_string(),
		language: root.then(|| origin.language.to_string()),
		entry_point: root.then(|| injection.entry_point().tag().to_string()),
		has_error: root.then(|| node.has_error()),
		named: node.is_named(),
		error: node.is_error(),
		missing: node.is_missing(),
		byte_range: (document_start, document_end),
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
			origin
				.column
				.saturating_add(point.column.saturating_sub(origin.prefix))
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
