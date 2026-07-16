use code_moniker_query::{
	WorkspaceGeneration, format_query_response_projected, parse_query, query_capability_spec,
	query_projection,
};
use serde_json::{Value, json};

use super::common::{apply_response_aliases, compact_argument};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
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
		"workspace generation; repeated monikers share one response-local alias table. ",
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
				"compact": {
					"type": "boolean",
					"default": true,
					"description": "Compact agent text by default; false returns the canonical typed JSON response."
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

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		execute_query(context, arguments)
	}
}

fn execute_query(context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
	let compact = compact_argument(arguments).map_err(ToolError::failed)?;
	let expressions = query_expressions(arguments).map_err(ToolError::failed)?;
	let mut outputs = Vec::with_capacity(expressions.len());
	let mut generation = None;
	let mut partial = false;
	for (index, expression) in expressions.iter().enumerate() {
		let request = parse_query(expression).map_err(ToolError::failed)?;
		let capability = request.query.capability();
		let projection = query_projection(&request.query).to_vec();
		let spec = query_capability_spec(capability)
			.ok_or_else(|| ToolError::failed(format!("query `{capability}` is not registered")))?;
		if !spec.read_only {
			return Err(ToolError::failed(format!(
				"query `{capability}` is not declared read-only; use its dedicated MCP tool"
			)));
		}
		let response = context.query(request).map_err(ToolError::failed)?;
		if let Some(observed) = response.generation {
			ensure_generation(&mut generation, observed).map_err(ToolError::failed)?;
		}
		partial |= response.next_cursor.is_some();
		let body = if compact {
			format_query_response_projected(&response, &projection)
		} else {
			serde_json::to_string_pretty(&response).map_err(ToolError::failed)?
		};
		if expressions.len() == 1 {
			outputs.push(format!("operation: {capability}\n\n{body}"));
		} else {
			outputs.push(format!(
				"result: {}\noperation: {capability}\n\n{body}",
				index + 1
			));
		}
	}
	let completeness = if partial {
		"partial (cursor available)"
	} else {
		"full"
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
	Ok(ToolResult {
		text: apply_response_aliases(output, compact, candidates.iter().map(String::as_str)),
		is_error: false,
	})
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
	fn extracts_monikers_for_response_local_compaction() {
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
