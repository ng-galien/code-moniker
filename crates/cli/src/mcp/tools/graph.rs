use std::fmt::Write as _;

use code_moniker_query::{
	Page, Query, QueryRequest, QueryResult, SymbolGraphFocus, SymbolGraphNeighbor,
	SymbolGraphQuery, SymbolGraphResult,
};
use serde_json::{Value, json};

use super::{McpTool, ToolDescriptor, ToolError, ToolResult};

use crate::mcp::context::McpContext;

pub(super) struct GraphTool;

impl GraphTool {
	pub(super) const NAME: &'static str = "code_moniker_graph";

	const DESCRIPTION: &'static str = concat!(
		"When to use: understand a symbol or file through its call-graph ",
		"neighborhood - who calls it, what it calls outside itself, and its ",
		"internal structure - before editing or reviewing it.\n",
		"\n",
		"Ego-centric unit neighborhood from code-moniker.\n",
		"The focus (symbol URI or workspace-relative file path) defines a unit ",
		"boundary; resolved references partition into internal edges, callers ",
		"(outside-in) and callees (inside-out), aggregated per neighbor with ",
		"relation kinds and call counts. Unresolved references are counted, ",
		"never dropped."
	);

	const DEFAULT_MAX_ITEMS: usize = 40;

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"focus": {
					"type": "string",
					"description": "Symbol URI (code+moniker://...) or workspace-relative file path."
				},
				"max_items": {
					"type": "integer",
					"minimum": 1,
					"maximum": 500,
					"description": "Bound for listed neighbors and members. Defaults 40; truncation is reported."
				}
			},
			"required": ["focus"],
			"additionalProperties": false
		})
	}
}

impl McpTool for GraphTool {
	fn descriptor(&self) -> ToolDescriptor {
		ToolDescriptor {
			name: Self::NAME,
			description: Self::DESCRIPTION,
			input_schema: Self::input_schema(),
		}
	}

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let Some(focus) = arguments.get("focus").and_then(Value::as_str) else {
			return Err(ToolError::failed(anyhow::anyhow!("focus is required")));
		};
		let max_items = arguments
			.get("max_items")
			.and_then(Value::as_u64)
			.map(|value| value as usize)
			.unwrap_or(Self::DEFAULT_MAX_ITEMS);
		let response = context
			.query(QueryRequest {
				query: Query::SymbolGraph(SymbolGraphQuery {
					workspace: None,
					focus: focus.to_string(),
				}),
				consistency: code_moniker_query::Consistency::RefreshIfStale,
				page: Page::default(),
			})
			.map_err(ToolError::failed)?;
		let QueryResult::SymbolGraph(result) = response.result else {
			return Err(ToolError::failed(anyhow::anyhow!(
				"unexpected symbol graph response"
			)));
		};
		Ok(ToolResult {
			text: render_graph(&result, max_items),
			is_error: false,
		})
	}
}

fn render_graph(result: &SymbolGraphResult, max_items: usize) -> String {
	let mut out = String::new();
	match &result.focus {
		SymbolGraphFocus::Symbol { symbol } => {
			let _ = writeln!(
				out,
				"focus: {} {} ({})",
				symbol.kind, symbol.name, symbol.file
			);
		}
		SymbolGraphFocus::File { path } => {
			let _ = writeln!(out, "focus: file {path}");
		}
	}
	let _ = writeln!(
		out,
		"members: {} internal edges: {}",
		result.members.len(),
		result.internal_edges.len()
	);
	let _ = writeln!(
		out,
		"unlinked refs: external {} · manifest-blocked {} · unresolved {}",
		result.unlinked.external, result.unlinked.manifest_blocked, result.unlinked.unresolved
	);
	if !result.unlinked.unresolved_reasons.is_empty() {
		let reasons = result
			.unlinked
			.unresolved_reasons
			.iter()
			.map(|(reason, count)| format!("{reason} {count}"))
			.collect::<Vec<_>>()
			.join(" · ");
		let _ = writeln!(out, "unresolved by reason: {reasons}");
	}
	render_neighbors(&mut out, "callers", &result.callers, max_items);
	render_neighbors(&mut out, "callees", &result.callees, max_items);
	out
}

fn render_neighbors(
	out: &mut String,
	label: &str,
	neighbors: &[SymbolGraphNeighbor],
	max_items: usize,
) {
	let _ = writeln!(out, "{label}: {}", neighbors.len());
	for neighbor in neighbors.iter().take(max_items) {
		let _ = writeln!(
			out,
			"- {} {} ({}) x{} [{}]",
			neighbor.symbol.kind,
			neighbor.symbol.name,
			neighbor.symbol.file,
			neighbor.count,
			neighbor.kinds.join(",")
		);
	}
	if neighbors.len() > max_items {
		let _ = writeln!(out, "- truncated: +{}", neighbors.len() - max_items);
	}
}
