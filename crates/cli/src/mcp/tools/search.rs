use code_moniker_query::{Query, QueryRequest, QueryResult, SymbolDto, SymbolSearchQuery};
use code_moniker_workspace::snapshot::{SourceFileRecord, SymbolRecord};
use serde_json::{Value, json};

use super::common::{line_range_suffix, symbol_line_suffix};
use super::scope::{
	Paging, SymbolScopeFilter, append_call_bool_arg, append_call_cursor_arg,
	append_call_number_arg, append_call_string_arg,
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
	let response = context.query(QueryRequest {
		query: Query::SymbolSearch(SymbolSearchQuery {
			workspace: None,
			text: Some(request.query.clone()),
			path: request.scope.files.paths.to_owned(),
			lang: request.scope.files.langs.to_owned(),
			kind: request.scope.kinds.to_owned(),
			shape: request
				.scope
				.shapes
				.iter()
				.map(|shape| shape.as_str().to_string())
				.collect(),
			name: request
				.scope
				.name
				.as_ref()
				.map(|regex| regex.as_str().to_string()),
			include_non_navigable: false,
			include_code: request.include_code,
			context_lines: request.context_lines,
			projection: Vec::new(),
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: request.paging.daemon_page(),
	})?;
	let QueryResult::SymbolList(result) = response.result else {
		anyhow::bail!("unexpected daemon response for search");
	};
	Ok(render_daemon_search_lmnav(
		context.scheme(),
		request,
		response.next_cursor.as_ref(),
		&result.rows,
		result.total,
	))
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

fn render_daemon_search_lmnav(
	scheme: &str,
	request: &SearchRequest,
	next_cursor: Option<&code_moniker_query::QueryCursor>,
	rows: &[SymbolDto],
	total: usize,
) -> String {
	let start = request.paging.cursor.min(total);
	let end = start.saturating_add(rows.len()).min(total);
	let mut output = String::new();
	output.push_str(&format!("uri: {scheme}workspace/search\n"));
	if let Some(next) = next_cursor {
		output.push_str(&format!(
			"completeness: partial (hits {start}-{end} of {total}, next cursor {})\n",
			next.offset
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("hits: {total}\n"));
	output.push_str(&format!("limit: {}\n\n", request.paging.limit));
	output.push_str("scope:\n");
	for line in request.scope.describe() {
		output.push_str(&line);
		output.push('\n');
	}
	output.push_str(&format!("  query: {}\n\n", request.query));
	output.push_str("results:\n");
	if rows.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for row in rows {
			render_daemon_search_row(&mut output, row);
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = next_cursor {
		append_daemon_search_next_call(&mut output, request, request.paging.limit, next);
	}
	append_search_next_call(&mut output, request, 50, None);
	output
}

fn render_daemon_search_row(output: &mut String, row: &SymbolDto) {
	output.push_str(&format!(
		"  - {} {} {}{}\n",
		row.kind,
		row.name,
		row.file,
		line_range_suffix(row.line_range)
	));
	if let Some(score) = row.score {
		output.push_str(&format!("    score: {score}\n"));
	}
	if let Some(reason) = &row.match_reason {
		output.push_str(&format!("    reason: {reason}\n"));
	}
	output.push_str(&format!("    uri: {}\n", row.uri));
	if let Some(source) = &row.source {
		output.push_str("    code:\n");
		for line in &source.lines {
			output.push_str(&format!("      {:>4} | {}\n", line.number, line.text));
		}
	}
}

fn append_daemon_search_next_call(
	output: &mut String,
	request: &SearchRequest,
	limit: usize,
	cursor: &code_moniker_query::QueryCursor,
) {
	output.push_str("  - code_moniker_search");
	append_call_string_arg(output, "query", &request.query);
	request.scope.append_call_args(output);
	if request.include_code {
		append_call_bool_arg(output, "include_code", true);
		append_call_number_arg(output, "context_lines", request.context_lines);
	}
	append_call_number_arg(output, "limit", limit);
	append_call_cursor_arg(output, "cursor", cursor);
	output.push('\n');
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
