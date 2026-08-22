use code_moniker_query::{
	ChangeContextQuery, ChangeContextResult, DEFAULT_CHANGE_CONTEXT_ITEMS,
	MAX_CHANGE_CONTEXT_ITEMS, Page, Query, QueryResult, SymbolGraphFocus, format_query_response,
};
use serde_json::{Value, json};

use super::common::compact_argument;
use super::{McpTool, OutputContract, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

pub(super) struct ContextTool;

impl ContextTool {
	pub(super) const NAME: &'static str = "code_moniker_context";

	const DESCRIPTION: &'static str = concat!(
		"When to use: gather the bounded symbolic context required before changing a symbol or file. ",
		"This combines its graph neighborhood, active notes, applicable rules, worktree changes, ",
		"coverage counts, and canonical suggested checks in one snapshot-consistent response.\n\n",
		"Output is compact and hard-budgeted by default. Body monikers use the compact form; ",
		"generated follow-up calls preserve the active compact or canonical mode."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"focus": {
					"type": "string",
					"description": "Compact moniker, canonical symbol URI, symbol id, unique bare name, unambiguous lang:path.kind:name reference, or workspace-relative file path. Ambiguity returns candidates."
				},
				"profile": {
					"type": "string",
					"description": "Optional rule profile used to select applicable checks."
				},
				"max_items": {
					"type": "integer",
					"minimum": 1,
					"maximum": MAX_CHANGE_CONTEXT_ITEMS,
					"default": DEFAULT_CHANGE_CONTEXT_ITEMS,
					"description": "Per-section bound. Coverage reports emitted/total counts."
				}
			},
			"required": ["focus"],
			"additionalProperties": false
		})
	}
}

impl McpTool for ContextTool {
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
		run_context(context, arguments)
	}
}

fn run_context(context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
	let Some(focus) = arguments.get("focus").and_then(Value::as_str) else {
		return Err(ToolError::failed(anyhow::anyhow!("focus is required")));
	};
	let compact = compact_argument(arguments).map_err(ToolError::failed)?;
	let max_items = arguments
		.get("max_items")
		.and_then(Value::as_u64)
		.map(|value| value as usize)
		.unwrap_or(DEFAULT_CHANGE_CONTEXT_ITEMS);
	if !(1..=MAX_CHANGE_CONTEXT_ITEMS).contains(&max_items) {
		return Err(ToolError::failed(anyhow::anyhow!(
			"max_items must be between 1 and {MAX_CHANGE_CONTEXT_ITEMS}"
		)));
	}
	let response = context
		.query_refreshed(
			Query::ChangeContext(ChangeContextQuery {
				workspace: None,
				focus: focus.to_string(),
				profile: arguments
					.get("profile")
					.and_then(Value::as_str)
					.map(ToOwned::to_owned),
				max_items,
			}),
			Page::default(),
		)
		.map_err(ToolError::failed)?;
	let QueryResult::ChangeContext(result) = &response.result else {
		return Err(ToolError::failed(anyhow::anyhow!(
			"unexpected daemon response for change context"
		)));
	};
	let monikers = response_monikers(result);
	let body = if compact {
		format_query_response(&response)
	} else {
		serde_json::to_string_pretty(&response).map_err(ToolError::failed)?
	};
	let output = format!(
		"uri: code+moniker://workspace\ncompleteness: bounded (coverage below)\nmode: change.context\n\n{body}"
	);
	Ok(ToolResult::success(output).with_monikers(monikers))
}

fn response_monikers(result: &ChangeContextResult) -> Vec<&str> {
	let mut candidates = Vec::new();
	if let SymbolGraphFocus::Symbol { symbol } = &result.focus {
		candidates.push(symbol.uri.as_str());
	}
	for member in &result.graph.members {
		candidates.push(member.uri.as_str());
	}
	for neighbor in result
		.graph
		.callers
		.iter()
		.chain(result.graph.callees.iter())
	{
		candidates.push(neighbor.symbol.uri.as_str());
	}
	for edge in &result.graph.internal_edges {
		candidates.push(edge.source.as_str());
		candidates.push(edge.target.as_str());
	}
	for note in &result.notes {
		candidates.push(note.moniker.as_str());
	}
	for change in &result.changed_symbols {
		if let Some(old) = &change.old {
			candidates.push(old.identity.as_str());
		}
		if let Some(new) = &change.new {
			candidates.push(new.identity.as_str());
		}
	}
	candidates
}
