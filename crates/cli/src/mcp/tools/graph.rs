use std::fmt::Write as _;

use code_moniker_query::{
	Page, Query, QueryRequest, QueryResult, SymbolGraphFocus, SymbolGraphNeighbor,
	SymbolGraphQuery, SymbolGraphResult, UsageDirection,
};
use serde_json::{Value, json};

use super::scope::string_list;
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
		"never dropped. Filter with direction, relation and min_count before rendering."
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
				},
				"direction": {
					"type": "string",
					"enum": ["incoming", "outgoing", "both"],
					"default": "both",
					"description": "Keep callers, callees, or both."
				},
				"relation": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Optional relation kind(s), OR-combined, for example calls or uses_type."
				},
				"min_count": {
					"type": "integer",
					"minimum": 1,
					"default": 1,
					"description": "Only keep aggregated edges with at least this count."
				},
				"include_internal": {
					"type": "boolean",
					"default": true,
					"description": "Include edges whose two endpoints stay inside the focus boundary."
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
		let request = graph_request(arguments).map_err(ToolError::failed)?;
		run_graph(context, request).map_err(ToolError::failed)
	}
}

struct GraphRequest {
	focus: String,
	max_items: usize,
	direction: UsageDirection,
	relation: Vec<String>,
	min_count: usize,
	include_internal: bool,
}

fn graph_request(arguments: &Value) -> anyhow::Result<GraphRequest> {
	let focus = arguments
		.get("focus")
		.and_then(Value::as_str)
		.ok_or_else(|| anyhow::anyhow!("focus is required"))?;
	let max_items = optional_u64(arguments, "max_items")?
		.map(|value| value as usize)
		.unwrap_or(GraphTool::DEFAULT_MAX_ITEMS);
	if !(1..=500).contains(&max_items) {
		anyhow::bail!("max_items must be between 1 and 500");
	}
	let direction = match arguments.get("direction") {
		Some(Value::String(value)) => value.parse::<UsageDirection>()?,
		Some(_) => anyhow::bail!("direction must be a string"),
		None => UsageDirection::Both,
	};
	let relation = string_list(arguments, "relation")?;
	let min_count = optional_u64(arguments, "min_count")?
		.map(|value| value as usize)
		.unwrap_or(1);
	if min_count == 0 {
		anyhow::bail!("min_count must be at least 1");
	}
	let include_internal = match arguments.get("include_internal") {
		Some(Value::Bool(value)) => *value,
		Some(_) => anyhow::bail!("include_internal must be a boolean"),
		None => true,
	};
	Ok(GraphRequest {
		focus: focus.to_string(),
		max_items,
		direction,
		relation,
		min_count,
		include_internal,
	})
}

fn optional_u64(arguments: &Value, name: &str) -> anyhow::Result<Option<u64>> {
	match arguments.get(name) {
		Some(Value::Number(value)) => value
			.as_u64()
			.map(Some)
			.ok_or_else(|| anyhow::anyhow!("{name} must be an unsigned integer")),
		Some(_) => anyhow::bail!("{name} must be an unsigned integer"),
		None => Ok(None),
	}
}

fn run_graph(context: &McpContext, request: GraphRequest) -> anyhow::Result<ToolResult> {
	let response = context.query(QueryRequest {
		query: Query::SymbolGraph(SymbolGraphQuery {
			workspace: None,
			focus: request.focus,
			direction: request.direction,
			relation: request.relation,
			min_count: request.min_count,
			include_internal: request.include_internal,
		}),
		consistency: code_moniker_query::Consistency::RefreshIfStale,
		page: Page::default(),
	})?;
	let QueryResult::SymbolGraph(result) = response.result else {
		anyhow::bail!("unexpected symbol graph response");
	};
	Ok(ToolResult {
		text: render_graph(&result, request.max_items),
		is_error: false,
	})
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
