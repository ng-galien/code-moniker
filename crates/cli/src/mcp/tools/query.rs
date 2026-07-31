use code_moniker_query::{
	QueryCursor, WorkspaceGeneration, format_query_response_projected, parse_query,
	query_capability_spec, query_projection,
};
use serde_json::{Value, json};

use super::common::compact_argument;
use super::scope::{
	append_call_bool_arg, append_call_cursor_arg, append_call_string_arg, cursor_argument,
};
use super::{McpTool, OutputContract, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

pub(super) struct QueryTool;

impl QueryTool {
	pub(super) const NAME: &'static str = "code_moniker_query";

	const DESCRIPTION: &'static str = concat!(
		"When to use: advanced read-only Code Moniker capability that is not covered ",
		"by read, symbols, usages, graph, rules, or diff. Prefer those intent tools ",
		"for normal exploration.\n\n",
		"Executes the daemon Query DSL through MCP, so agents never need a direct ",
		"daemon or shell fallback. Use query.describe to discover the live grammar. ",
		"Pass queries for a bounded batch of up to four read-only operations at one ",
		"workspace generation; compact responses shorten every rendered moniker. ",
		"Paginated results include an executable next call that preserves the original ",
		"query and resumes it with the generation-aware cursor. ",
		"Mutating or mixed queries such as notes are rejected here and remain behind ",
		"their dedicated MCP tool. Output is compact and hard-budgeted by default."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"query": {
					"type": "string",
					"description": "Bounded Code Moniker Query DSL expression. Start with `query.describe` or `query.describe verb:\"identity.graph\"`."
				},
				"queries": {
					"type": "array",
					"items": { "type": "string" },
					"minItems": 1,
					"maxItems": 4,
					"description": "Bounded read-only batch. Every result must observe the same workspace generation."
				},
				"cursor": {
					"oneOf": [
						{ "type": "integer", "minimum": 0 },
						{ "type": "string", "pattern": "^[0-9]+:[0-9]+$" }
					],
					"description": "Resume a single query at an offset or generation-aware cursor. Overrides any cursor embedded in the query expression."
				}
			},
			"oneOf": [
				{ "required": ["query"] },
				{ "required": ["queries"] }
			],
			"additionalProperties": false
		})
	}
}

impl McpTool for QueryTool {
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

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		execute_query(context, arguments)
	}
}

fn execute_query(context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
	let compact = compact_argument(arguments).map_err(ToolError::failed)?;
	let expressions = query_expressions(arguments).map_err(ToolError::failed)?;
	let cursor = cursor_argument(arguments, "cursor")
		.map_err(ToolError::failed)?
		.map(|(offset, generation)| QueryCursor::new(offset, generation));
	if cursor.is_some() && expressions.len() != 1 {
		return Err(ToolError::failed(anyhow::anyhow!(
			"`cursor` can only resume a single `query`, not a `queries` batch"
		)));
	}
	let mut outputs = Vec::with_capacity(expressions.len());
	let mut generation = None;
	let mut partial = false;
	let mut errors = 0usize;
	for (index, expression) in expressions.iter().enumerate() {
		match run_expression(
			context,
			expression,
			cursor.as_ref(),
			compact,
			&mut generation,
		) {
			Ok((capability, body, next_cursor)) => {
				partial |= next_cursor.is_some();
				let body = append_query_next(body, expression, next_cursor.as_ref(), compact);
				if expressions.len() == 1 {
					outputs.push(format!("operation: {capability}\n\n{body}"));
				} else {
					outputs.push(format!(
						"result: {}\noperation: {capability}\n\n{body}",
						index + 1
					));
				}
			}
			Err(error) if expressions.len() == 1 => return Err(ToolError::failed(error)),
			Err(error) => {
				errors += 1;
				outputs.push(format!("result: {}\nerror: {error:#}", index + 1));
			}
		}
	}
	let completeness = if errors > 0 {
		format!("partial ({errors} error(s))")
	} else if partial {
		"partial (cursor available)".to_string()
	} else {
		"full".to_string()
	};
	let operation = if expressions.len() == 1 {
		"query"
	} else {
		"query.batch"
	};
	let output = format!(
		"uri: code+moniker://workspace\ncompleteness: {completeness}\nmode: {operation}\n\n{}",
		outputs.join("\n---\n")
	);
	let candidates = moniker_candidates(&output);
	Ok(ToolResult::success(output).with_monikers(candidates.iter().map(String::as_str)))
}

fn run_expression(
	context: &McpContext,
	expression: &str,
	cursor: Option<&QueryCursor>,
	compact: bool,
	generation: &mut Option<WorkspaceGeneration>,
) -> anyhow::Result<(&'static str, String, Option<QueryCursor>)> {
	let mut request = parse_query(expression)?;
	if let Some(cursor) = cursor {
		request.page.cursor = Some(cursor.clone());
	}
	let capability = request.query.capability();
	let projection = query_projection(&request.query).to_vec();
	let spec = query_capability_spec(capability)
		.ok_or_else(|| anyhow::anyhow!("query `{capability}` is not registered"))?;
	if !spec.read_only {
		anyhow::bail!("query `{capability}` is not declared read-only; use its dedicated MCP tool");
	}
	let response = context.query(request)?;
	if let Some(observed) = response.generation {
		ensure_generation(generation, observed)?;
	}
	let next_cursor = response.next_cursor.clone();
	let body = if compact {
		format_query_response_projected(&response, &projection)
	} else {
		serde_json::to_string_pretty(&response)?
	};
	Ok((capability, body, next_cursor))
}

fn append_query_next(
	mut body: String,
	expression: &str,
	next_cursor: Option<&QueryCursor>,
	compact: bool,
) -> String {
	let Some(next_cursor) = next_cursor else {
		return body;
	};
	body.push_str("\nnext:\n  - code_moniker_query");
	append_call_string_arg(&mut body, "query", expression);
	append_call_cursor_arg(&mut body, "cursor", next_cursor);
	if !compact {
		append_call_bool_arg(&mut body, "compact", false);
	}
	body.push('\n');
	body
}

fn query_expressions(arguments: &Value) -> anyhow::Result<Vec<&str>> {
	let query = arguments.get("query");
	let queries = arguments.get("queries");
	if query.is_some() && queries.is_some() {
		anyhow::bail!("pass either `query` or `queries`, not both");
	}
	if let Some(query) = query {
		return query
			.as_str()
			.map(|query| vec![query])
			.ok_or_else(|| anyhow::anyhow!("`query` must be a string"));
	}
	let Some(queries) = queries.and_then(Value::as_array) else {
		anyhow::bail!("`query` or `queries` is required");
	};
	if queries.is_empty() || queries.len() > 4 {
		anyhow::bail!("`queries` must contain between 1 and 4 expressions");
	}
	queries
		.iter()
		.map(|query| {
			query
				.as_str()
				.ok_or_else(|| anyhow::anyhow!("every `queries` item must be a string"))
		})
		.collect()
}

fn ensure_generation(
	current: &mut Option<WorkspaceGeneration>,
	observed: WorkspaceGeneration,
) -> anyhow::Result<()> {
	match current {
		Some(current) if *current != observed => anyhow::bail!(
			"workspace generation changed during query batch ({} -> {}); retry",
			current.0,
			observed.0
		),
		Some(_) => Ok(()),
		None => {
			*current = Some(observed);
			Ok(())
		}
	}
}

fn moniker_candidates(output: &str) -> Vec<String> {
	let mut candidates = Vec::new();
	for token in output.split_whitespace() {
		let Some(start) = token.find("code+moniker://") else {
			continue;
		};
		let token = token[start..].trim_matches(|ch: char| matches!(ch, '"' | '\'' | ',' | ';'));
		if token.starts_with("code+moniker://") {
			candidates.push(token.to_string());
		}
	}
	candidates
}

#[cfg(test)]
mod tests {
	use super::moniker_candidates;

	#[test]
	fn extracts_monikers_for_compact_rendering() {
		let uri = "code+moniker://./lang:rs/module:lib/fn:run()";
		assert_eq!(
			moniker_candidates(&format!("uri: {uri}\ntarget: {uri}\n")),
			vec![uri, uri]
		);
		assert_eq!(
			moniker_candidates(&format!("- uri={uri}\n- uri={uri}\n")),
			vec![uri, uri]
		);
	}
}
