use std::collections::BTreeMap;

use code_moniker_workspace::snapshot::{SourceFileRecord, SymbolRecord, WorkspaceView};
use serde_json::{Value, json};

use super::common::symbol_line_suffix;
use super::scope::{
	Paging, SymbolMatch, SymbolScopeFilter, append_call_bool_arg, append_call_number_arg,
	append_call_string_arg,
};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

const DEFAULT_CONTEXT_LINES: usize = 0;
const MAX_CONTEXT_LINES: usize = 20;

pub(super) struct SearchTool;

impl SearchTool {
	pub(super) const NAME: &'static str = "code_moniker_search";

	const DESCRIPTION: &'static str = concat!(
		"When to use: search symbols using the same workspace search index as the TUI header search. ",
		"Use code_moniker_symbols when you need exact regex filtering over symbol names instead.\n",
		"\n",
		"Search from code-moniker.\n",
		"  query — fuzzy symbol search text, scored like the TUI search\n",
		"  path/lang/kind/shape — same filters as code_moniker_symbols\n",
		"  include_code/context_lines — opt into source lines around each symbol range\n",
		"Use limit and cursor for paging; next calls preserve the search query and scope."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"query": {
					"type": "string",
					"description": "Fuzzy symbol search text, scored like the TUI search."
				},
				"path": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Relative file glob(s), OR-combined."
				},
				"lang": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Language tag(s), OR-combined. Example: rs, java"
				},
				"kind": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Concrete symbol kind(s), OR-combined. Example: class, interface, fn, method"
				},
				"shape": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Shape family, OR-combined. One of namespace,type,callable,value,annotation,ref"
				},
				"name": {
					"type": "string",
					"description": "Rust regex matched against symbol name after TUI-style search scoring."
				},
				"include_code": {
					"type": "boolean",
					"description": "Include source lines for each hit. Defaults false for terse search results."
				},
				"context_lines": {
					"type": "integer",
					"minimum": 0,
					"maximum": MAX_CONTEXT_LINES,
					"description": "Extra source lines around each matched symbol range when include_code is true."
				},
				"limit": {
					"type": "integer",
					"minimum": 1,
					"maximum": super::scope::MAX_LIMIT,
					"description": "Maximum search hits to emit."
				},
				"cursor": {
					"oneOf": [{ "type": "integer" }, { "type": "string" }],
					"description": "Opaque row offset returned in next calls."
				}
			},
			"required": ["query"],
			"additionalProperties": false
		})
	}
}

impl McpTool for SearchTool {
	fn descriptor(&self) -> ToolDescriptor {
		ToolDescriptor {
			name: Self::NAME,
			description: Self::DESCRIPTION,
			input_schema: Self::input_schema(),
		}
	}

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let request = SearchRequest::from_arguments(arguments).map_err(ToolError::failed)?;
		let text = search_symbols(context, &request).map_err(ToolError::failed)?;
		Ok(ToolResult {
			text,
			is_error: false,
		})
	}
}

struct SearchRequest {
	query: String,
	scope: SymbolScopeFilter,
	paging: Paging,
	include_code: bool,
	context_lines: usize,
}

impl SearchRequest {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		if arguments.get("include_non_navigable").is_some() {
			anyhow::bail!("`include_non_navigable` is unsupported by TUI-style search");
		}
		Ok(Self {
			query: arguments
				.get("query")
				.and_then(Value::as_str)
				.ok_or_else(|| anyhow::anyhow!("`query` is required"))?
				.to_string(),
			scope: SymbolScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments(arguments)?,
			include_code: arguments
				.get("include_code")
				.and_then(Value::as_bool)
				.unwrap_or(false),
			context_lines: arguments
				.get("context_lines")
				.and_then(Value::as_u64)
				.unwrap_or(DEFAULT_CONTEXT_LINES as u64)
				.min(MAX_CONTEXT_LINES as u64) as usize,
		})
	}
}

struct SearchRow<'a> {
	symbol: &'a SymbolRecord,
	source: &'a SourceFileRecord,
	score: u32,
	reason: String,
	code_lines: Vec<(usize, String)>,
}

fn search_symbols(context: &McpContext, request: &SearchRequest) -> anyhow::Result<String> {
	let snapshot = context.index().index_snapshot()?;
	let source_by_id = snapshot
		.index
		.sources
		.iter()
		.map(|source| (source.id.clone(), source))
		.collect::<BTreeMap<_, _>>();
	let symbol_by_id = snapshot
		.index
		.symbols
		.iter()
		.map(|symbol| (symbol.id.clone(), symbol))
		.collect::<BTreeMap<_, _>>();
	let mut rows = Vec::new();
	for hit in WorkspaceView::new(snapshot.as_ref())
		.search()
		.search_symbols_matching(&request.query, search_candidate_limit(request), |symbol| {
			let Some(source) = source_by_id.get(&symbol.source) else {
				return false;
			};
			request
				.scope
				.files
				.matches_file(&source.rel_path, Some(&source.language))
				&& request.scope.matches_tui_search_symbol(SymbolMatch {
					name: &symbol.name,
					kind: &symbol.kind,
					navigable: symbol.navigable,
				})
		}) {
		let Some(symbol) = symbol_by_id.get(&hit.symbol) else {
			continue;
		};
		let Some(source) = source_by_id.get(&symbol.source) else {
			continue;
		};
		rows.push(SearchRow {
			symbol,
			source,
			score: hit.score,
			reason: hit.reason,
			code_lines: if request.include_code {
				symbol_source_lines(source, symbol, request.context_lines)
			} else {
				Vec::new()
			},
		});
	}
	Ok(render_search_lmnav(context.scheme(), request, &rows))
}

fn search_candidate_limit(request: &SearchRequest) -> usize {
	request
		.paging
		.cursor
		.saturating_add(request.paging.limit)
		.saturating_add(1)
}

fn symbol_source_lines(
	source: &SourceFileRecord,
	symbol: &SymbolRecord,
	context_lines: usize,
) -> Vec<(usize, String)> {
	let Some((start, end)) = symbol.line_range else {
		return Vec::new();
	};
	let source_text = if source.text.is_empty() {
		match std::fs::read_to_string(&source.path) {
			Ok(text) => text,
			Err(_) => return Vec::new(),
		}
	} else {
		source.text.clone()
	};
	let lines = source_text.lines().collect::<Vec<_>>();
	if lines.is_empty() {
		return Vec::new();
	}
	let slice_start = (start as usize).saturating_sub(context_lines).max(1);
	let slice_end = (end as usize)
		.saturating_add(context_lines)
		.min(lines.len());
	(slice_start..=slice_end)
		.map(|number| (number, lines[number - 1].to_string()))
		.collect()
}

fn render_search_lmnav(scheme: &str, request: &SearchRequest, rows: &[SearchRow<'_>]) -> String {
	let (start, end, next) = request.paging.window(rows);
	let mut output = String::new();
	output.push_str(&format!("uri: {scheme}workspace/search\n"));
	if let Some(next) = next {
		output.push_str(&format!(
			"completeness: partial (hits {start}-{end} of {}, next cursor {next})\n",
			rows.len()
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("hits: {}\n", rows.len()));
	output.push_str(&format!("limit: {}\n", request.paging.limit));
	output.push('\n');
	output.push_str("scope:\n");
	for line in request.scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push_str(&format!("  query: {}\n", request.query));
	output.push('\n');
	output.push_str("results:\n");
	if rows.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for row in rows.iter().take(end).skip(start) {
			render_search_row(&mut output, row);
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = next {
		append_search_next_call(&mut output, request, request.paging.limit, Some(next));
	}
	append_search_next_call(&mut output, request, 50, None);
	output
}

fn render_search_row(output: &mut String, row: &SearchRow<'_>) {
	output.push_str(&format!(
		"  - {} {} {}{}\n",
		row.symbol.kind,
		row.symbol.name,
		row.source.rel_path,
		symbol_line_suffix(row.symbol)
	));
	output.push_str(&format!("    score: {}\n", row.score));
	output.push_str(&format!("    reason: {}\n", row.reason));
	output.push_str(&format!("    uri: {}\n", row.symbol.identity));
	if !row.code_lines.is_empty() {
		output.push_str("    code:\n");
		for (line_number, line) in &row.code_lines {
			output.push_str(&format!("      {line_number:>4} | {line}\n"));
		}
	}
}

fn append_search_next_call(
	output: &mut String,
	request: &SearchRequest,
	limit: usize,
	cursor: Option<usize>,
) {
	output.push_str("  - code_moniker_search");
	append_call_string_arg(output, "query", &request.query);
	request.scope.append_call_args(output);
	if request.include_code {
		append_call_bool_arg(output, "include_code", true);
		append_call_number_arg(output, "context_lines", request.context_lines);
	}
	append_call_number_arg(output, "limit", limit);
	if let Some(cursor) = cursor {
		append_call_number_arg(output, "cursor", cursor);
	}
	output.push('\n');
}
