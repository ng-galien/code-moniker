use code_moniker_query::{Query, QueryResult, SymbolDto, SymbolSearchQuery};
use serde::Serialize;
use serde_json::{Value, json};

use super::common::{AgentOutputOptions, line_range_suffix};
use super::scope::{
	Paging, ScopeRowView, SymbolScopeFilter, append_call_bool_arg, append_call_cursor_arg,
	append_call_number_arg, append_call_string_arg, scope_rows,
};
use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;
#[cfg(test)]
use crate::presentation::RenderOptions;
use crate::presentation::{TemplateOutput, symbols as symbols_presentation};

const DEFAULT_CONTEXT_LINES: usize = 0;
const MAX_CONTEXT_LINES: usize = 20;
pub(super) struct SearchTool;

impl SearchTool {
	pub(super) const NAME: &'static str = "code_moniker_search";

	const DESCRIPTION: &'static str = concat!(
		"When to use: fuzzy-search symbols through the workspace search index. ",
		"Use code_moniker_symbols when you need exact regex filtering over symbol names instead.\n",
		"\n",
		"Search from code-moniker.\n",
		"  query — fuzzy symbol search text\n",
		"  path/lang/kind/shape — same filters as code_moniker_symbols\n",
		"  include_code/context_lines — opt into source lines around each symbol range\n",
		"Use limit and cursor for paging; compact output keeps only the cursor follow-up by default."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"query": {
					"type": "string",
					"description": "Fuzzy symbol search text."
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
					"description": "Rust regex matched against symbol name after fuzzy-search scoring."
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

	fn output_contract(&self) -> OutputContract {
		OutputContract::Agent
	}

	fn call(
		&self,
		context: &McpContext,
		arguments: &Value,
		output: OutputOptions,
	) -> Result<ToolResult, ToolError> {
		let request = SearchRequest::from_arguments(arguments, output.agent_options())
			.map_err(ToolError::failed)?;
		search_symbols(context, &request).map_err(ToolError::failed)
	}
}

struct SearchRequest {
	query: String,
	scope: SymbolScopeFilter,
	paging: Paging,
	include_code: bool,
	context_lines: usize,
	output: AgentOutputOptions,
}

impl SearchRequest {
	fn from_arguments(arguments: &Value, output: AgentOutputOptions) -> anyhow::Result<Self> {
		if arguments.get("include_non_navigable").is_some() {
			anyhow::bail!("`include_non_navigable` is unsupported by fuzzy search");
		}
		Ok(Self {
			query: arguments
				.get("query")
				.and_then(Value::as_str)
				.ok_or_else(|| anyhow::anyhow!("`query` is required"))?
				.to_string(),
			scope: SymbolScopeFilter::from_arguments(arguments)?,
			paging: Paging::from_arguments_for_volume(arguments, output)?,
			include_code: arguments
				.get("include_code")
				.and_then(Value::as_bool)
				.unwrap_or(false),
			context_lines: arguments
				.get("context_lines")
				.and_then(Value::as_u64)
				.unwrap_or(DEFAULT_CONTEXT_LINES as u64)
				.min(MAX_CONTEXT_LINES as u64) as usize,
			output,
		})
	}
}

fn search_symbols(context: &McpContext, request: &SearchRequest) -> anyhow::Result<ToolResult> {
	let response = context.query_refreshed(
		Query::SymbolSearch(SymbolSearchQuery {
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
		request.paging.daemon_page(),
	)?;
	let QueryResult::SymbolList(result) = response.result else {
		anyhow::bail!("unexpected daemon response for search");
	};
	Ok(ToolResult::templated(daemon_search_template(
		context.scheme(),
		request,
		response.next_cursor.as_ref(),
		&result.rows,
		result.total,
	)?))
}

fn daemon_search_template(
	scheme: &str,
	request: &SearchRequest,
	next_cursor: Option<&code_moniker_query::QueryCursor>,
	rows: &[SymbolDto],
	total: usize,
) -> anyhow::Result<TemplateOutput> {
	let start = request.paging.cursor.min(total);
	let end = start.saturating_add(rows.len()).min(total);
	let mut next_calls = Vec::new();
	if let Some(next) = next_cursor {
		next_calls.push(SearchCallView {
			arguments: search_call_arguments(request, request.paging.limit, Some(next)),
		});
	}
	if !request.output.compact {
		next_calls.push(SearchCallView {
			arguments: search_call_arguments(request, request.output.default_page_limit(), None),
		});
	}
	let context = SearchView {
		uri: format!("{scheme}workspace/search"),
		partial: next_cursor.is_some(),
		start,
		end,
		total,
		next_cursor: next_cursor.map(|cursor| cursor.offset),
		limit: request.paging.limit,
		volume: request.output.budget.as_str(),
		scope: scope_rows(&request.scope),
		query: &request.query,
		rows: rows.iter().map(SearchRowView::from).collect(),
		zero_hit: total == 0,
		next_calls,
	};
	symbols_presentation::search(&context)
}

#[derive(Serialize)]
struct SearchView<'a> {
	uri: String,
	partial: bool,
	start: usize,
	end: usize,
	total: usize,
	next_cursor: Option<usize>,
	limit: usize,
	volume: &'static str,
	scope: Vec<ScopeRowView>,
	query: &'a str,
	rows: Vec<SearchRowView<'a>>,
	zero_hit: bool,
	next_calls: Vec<SearchCallView>,
}

#[derive(Serialize)]
struct SearchRowView<'a> {
	kind: &'a str,
	name: &'a str,
	location: String,
	score: Option<u32>,
	reason: Option<&'a str>,
	uri: &'a str,
	code: Vec<String>,
}

impl<'a> From<&'a SymbolDto> for SearchRowView<'a> {
	fn from(row: &'a SymbolDto) -> Self {
		Self {
			kind: &row.kind,
			name: &row.name,
			location: format!("{}{}", row.file, line_range_suffix(row.line_range)),
			score: row.score,
			reason: row.match_reason.as_deref(),
			uri: &row.uri,
			code: row
				.source
				.as_ref()
				.map(|source| {
					source
						.lines
						.iter()
						.map(|line| format!("{:>4} | {}", line.number, line.text))
						.collect()
				})
				.unwrap_or_default(),
		}
	}
}

#[derive(Serialize)]
struct SearchCallView {
	arguments: String,
}

fn search_call_arguments(
	request: &SearchRequest,
	limit: usize,
	cursor: Option<&code_moniker_query::QueryCursor>,
) -> String {
	let mut arguments = String::new();
	append_call_string_arg(&mut arguments, "query", &request.query);
	request.scope.append_call_args(&mut arguments);
	if request.include_code {
		append_call_bool_arg(&mut arguments, "include_code", true);
		append_call_number_arg(&mut arguments, "context_lines", request.context_lines);
	}
	append_call_number_arg(&mut arguments, "limit", limit);
	if let Some(cursor) = cursor {
		append_call_cursor_arg(&mut arguments, "cursor", cursor);
	}
	append_call_string_arg(&mut arguments, "budget", request.output.budget.as_str());
	if !request.output.compact {
		append_call_bool_arg(&mut arguments, "compact", false);
	}
	arguments
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn volume_profiles_shape_search_pages_before_rendering() {
		for (budget, expected) in [("small", 20), ("medium", 80), ("full", 500)] {
			let arguments = serde_json::json!({"query": "run", "budget": budget});
			let output = AgentOutputOptions::from_arguments(&arguments).expect("output options");
			let request =
				SearchRequest::from_arguments(&arguments, output).expect("search request");
			assert_eq!(request.paging.limit, expected, "{budget}");
		}

		let arguments = serde_json::json!({
			"query": "run",
			"budget": "medium",
			"compact": false
		});
		let output = AgentOutputOptions::from_arguments(&arguments).expect("output options");
		let request = SearchRequest::from_arguments(&arguments, output).expect("search request");
		let rendered = daemon_search_template("code+moniker://", &request, None, &[], 0)
			.expect("search template")
			.render(RenderOptions {
				compact: false,
				scheme: "code+moniker://",
				runtime: None,
			})
			.expect("rendered search");
		assert!(rendered.contains("page-size: 80"), "{rendered}");
		assert!(rendered.contains("budget=\"medium\""), "{rendered}");
		assert!(rendered.contains("compact=false"), "{rendered}");
	}

	#[test]
	fn zero_hit_search_explains_name_scoring() {
		let arguments = serde_json::json!({"query": "run check command"});
		let output = AgentOutputOptions::from_arguments(&arguments).expect("output options");
		let request = SearchRequest::from_arguments(&arguments, output).expect("search request");

		let rendered = daemon_search_template("code+moniker://", &request, None, &[], 0)
			.expect("search template")
			.render(RenderOptions {
				compact: true,
				scheme: "code+moniker://",
				runtime: None,
			})
			.expect("rendered search");

		assert!(
			rendered.contains("scores symbol names"),
			"a zero-hit search must explain name scoring, got:\n{rendered}"
		);
		assert!(
			rendered.contains("code_moniker_symbols"),
			"the hint must route to the regex tool, got:\n{rendered}"
		);
	}
}
