use std::fmt::Write as _;

use code_moniker_query::{
	GraphSectionCoverage, MAX_BOUNDED_RESULT_ITEMS, Page, Query, QueryResult, SymbolGraphFocus,
	SymbolGraphNeighbor, SymbolGraphQuery, SymbolGraphResult, UsageDirection,
};
use serde_json::{Value, json};

use super::scope::string_list;
use super::{McpTool, OutputContract, ToolDescriptor, ToolError, ToolResult};

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
		"relation kinds and call counts. Non-unique references remain outside ",
		"the graph and are classified, never dropped. Filter with direction, ",
		"relation and min_count before rendering. Coverage distinguishes total ",
		"facts before filters, matching facts after relational filters, and ",
		"returned facts after direction, internal-edge and output bounds."
	);

	const DEFAULT_MAX_ITEMS: usize = 40;

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"focus": {
					"type": "string",
					"description": "Compact moniker, canonical symbol URI, symbol id, unique bare name, unambiguous lang:path.kind:name reference, or workspace-relative file path. Ambiguity returns candidates."
				},
				"max_items": {
					"type": "integer",
					"minimum": 1,
					"maximum": MAX_BOUNDED_RESULT_ITEMS,
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

	fn output_contract(&self) -> OutputContract {
		OutputContract::Agent
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
	if !(1..=MAX_BOUNDED_RESULT_ITEMS).contains(&max_items) {
		anyhow::bail!("max_items must be between 1 and {MAX_BOUNDED_RESULT_ITEMS}");
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
	let GraphRequest {
		focus,
		max_items,
		direction,
		relation,
		min_count,
		include_internal,
	} = request;
	let response = context.query_refreshed(
		Query::SymbolGraph(SymbolGraphQuery {
			workspace: None,
			focus,
			direction,
			relation,
			min_count,
			include_internal,
			limit: max_items,
		}),
		Page::default(),
	)?;
	let QueryResult::SymbolGraph(result) = response.result else {
		anyhow::bail!("unexpected symbol graph response");
	};
	let candidates = graph_monikers(&result);
	Ok(ToolResult::success(render_graph(&result, max_items, direction)).with_monikers(candidates))
}

fn graph_monikers(result: &SymbolGraphResult) -> Vec<&str> {
	let mut monikers = Vec::new();
	if let SymbolGraphFocus::Symbol { symbol } = &result.focus {
		monikers.push(symbol.uri.as_str());
	}
	for member in &result.members {
		monikers.push(member.uri.as_str());
	}
	for edge in &result.internal_edges {
		monikers.push(edge.source.as_str());
		monikers.push(edge.target.as_str());
	}
	for neighbor in result.callers.iter().chain(&result.callees) {
		monikers.push(neighbor.symbol.uri.as_str());
	}
	monikers
}

fn render_graph(result: &SymbolGraphResult, max_items: usize, direction: UsageDirection) -> String {
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
		"members: {}/{} internal edges: {}/{} matching ({} total)",
		result.coverage.members.returned,
		result.coverage.members.total,
		result.coverage.internal_edges.returned,
		result.coverage.internal_edges.matching,
		result.coverage.internal_edges.total
	);
	let _ = writeln!(
		out,
		"unlinked refs: external {} (sdk {} · dependency {} · injected {} · unknown {}) · candidate {} · dynamic {} · manifest-blocked {} · unresolved {}",
		result.unlinked.external,
		result.unlinked.sdk,
		result.unlinked.dependency,
		result.unlinked.injected_external,
		result.unlinked.unknown_external,
		result.unlinked.candidate,
		result.unlinked.dynamic,
		result.unlinked.manifest_blocked,
		result.unlinked.unresolved
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
	if direction != UsageDirection::Outgoing {
		render_neighbors(
			&mut out,
			"callers",
			&result.callers,
			result.coverage.callers,
			max_items,
		);
	}
	if direction != UsageDirection::Incoming {
		render_neighbors(
			&mut out,
			"callees",
			&result.callees,
			result.coverage.callees,
			max_items,
		);
	}
	out
}

fn render_neighbors(
	out: &mut String,
	label: &str,
	neighbors: &[SymbolGraphNeighbor],
	coverage: GraphSectionCoverage,
	max_items: usize,
) {
	let shown = neighbors.len().min(max_items);
	let _ = writeln!(
		out,
		"{label}: {shown}/{} matching ({} total)",
		coverage.matching, coverage.total
	);
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
		if neighbor.symbol.navigable {
			let _ = writeln!(out, "  uri: {}", neighbor.symbol.uri);
		}
	}
	if neighbors.len() > max_items {
		let _ = writeln!(out, "- truncated: +{}", neighbors.len() - max_items);
	}
}

#[cfg(test)]
mod tests {
	use code_moniker_query::{GraphSectionCoverage, SymbolGraphCoverage, UnlinkedRefsDto};

	use super::*;

	#[test]
	fn graph_render_classifies_non_unique_references_outside_the_graph() {
		let result = SymbolGraphResult {
			focus: SymbolGraphFocus::File {
				path: "src/sample.py".to_string(),
			},
			direction: UsageDirection::Both,
			coverage: Default::default(),
			members: Vec::new(),
			internal_edges: Vec::new(),
			callers: Vec::new(),
			callees: Vec::new(),
			unlinked: UnlinkedRefsDto {
				external: 1,
				sdk: 1,
				dependency: 0,
				injected_external: 0,
				unknown_external: 0,
				candidate: 2,
				dynamic: 3,
				manifest_blocked: 4,
				unresolved: 5,
				unresolved_reasons: Default::default(),
			},
		};

		let rendered = render_graph(&result, 10, UsageDirection::Both);

		assert!(rendered.contains(
			"unlinked refs: external 1 (sdk 1 · dependency 0 · injected 0 · unknown 0) · candidate 2 · dynamic 3 · manifest-blocked 4 · unresolved 5"
		));
	}

	#[test]
	fn graph_render_omits_sections_filtered_by_direction() {
		let result = SymbolGraphResult {
			focus: SymbolGraphFocus::File {
				path: "src/sample.py".to_string(),
			},
			direction: UsageDirection::Outgoing,
			coverage: Default::default(),
			members: Vec::new(),
			internal_edges: Vec::new(),
			callers: Vec::new(),
			callees: Vec::new(),
			unlinked: UnlinkedRefsDto::default(),
		};

		let rendered = render_graph(&result, 10, UsageDirection::Outgoing);

		assert!(
			!rendered.contains("callers:"),
			"a caller section cleared by direction=outgoing must not render as a fact, got:\n{rendered}"
		);
		assert!(
			rendered.contains("callees: 0"),
			"the requested direction must still render, got:\n{rendered}"
		);
	}

	#[test]
	fn graph_render_labels_zero_after_filter_with_the_prefilter_total() {
		let result = SymbolGraphResult {
			focus: SymbolGraphFocus::File {
				path: "src/hub.rs".to_string(),
			},
			direction: UsageDirection::Incoming,
			coverage: SymbolGraphCoverage {
				callers: GraphSectionCoverage {
					total: 2_192,
					matching: 0,
					returned: 0,
				},
				..Default::default()
			},
			members: Vec::new(),
			internal_edges: Vec::new(),
			callers: Vec::new(),
			callees: Vec::new(),
			unlinked: UnlinkedRefsDto::default(),
		};

		let rendered = render_graph(&result, 10, UsageDirection::Incoming);

		assert!(
			rendered.contains("callers: 0/0 matching (2192 total)"),
			"a post-filter zero must retain its pre-filter denominator:\n{rendered}"
		);
	}

	#[test]
	fn graph_render_keeps_canonical_neighbor_uris() {
		let neighbor_uri = "code+moniker://./lang:rs/module:sample/fn:caller()".to_string();
		let local_uri =
			"code+moniker://./lang:rs/module:sample/fn:caller()/local:value".to_string();
		let result = SymbolGraphResult {
			focus: SymbolGraphFocus::File {
				path: "src/sample.rs".to_string(),
			},
			direction: UsageDirection::Both,
			coverage: Default::default(),
			members: Vec::new(),
			internal_edges: Vec::new(),
			callers: vec![
				SymbolGraphNeighbor {
					symbol: code_moniker_query::SymbolDto {
						root: "/workspace".to_string(),
						uri: neighbor_uri.clone(),
						id: "symbol:1:0".to_string(),
						name: "caller()".to_string(),
						kind: "fn".to_string(),
						visibility: "private".to_string(),
						signature: String::new(),
						file: "src/caller.rs".to_string(),
						language: "rs".to_string(),
						line_range: Some((1, 3)),
						navigable: true,
						score: None,
						match_reason: None,
						source: None,
					},
					kinds: vec!["calls".to_string()],
					count: 1,
				},
				SymbolGraphNeighbor {
					symbol: code_moniker_query::SymbolDto {
						root: "/workspace".to_string(),
						uri: local_uri.clone(),
						id: "symbol:1:1".to_string(),
						name: "value".to_string(),
						kind: "local".to_string(),
						visibility: "private".to_string(),
						signature: String::new(),
						file: "src/caller.rs".to_string(),
						language: "rs".to_string(),
						line_range: Some((2, 2)),
						navigable: false,
						score: None,
						match_reason: None,
						source: None,
					},
					kinds: vec!["reads".to_string()],
					count: 1,
				},
			],
			callees: Vec::new(),
			unlinked: UnlinkedRefsDto::default(),
		};

		let rendered = render_graph(&result, 10, UsageDirection::Incoming);

		assert!(
			rendered.contains(&format!("  uri: {neighbor_uri}")),
			"{rendered}"
		);
		assert!(
			!rendered.contains(&format!("  uri: {local_uri}")),
			"{rendered}"
		);
	}
}
