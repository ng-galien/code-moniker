use code_moniker_query::{
	MAX_BOUNDED_RESULT_ITEMS, Page, Query, QueryResult, SymbolGraphQuery, SymbolGraphResult,
	UsageDirection,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::common::{AgentOutputOptions, OutputBudget};
use super::scope::{
	append_call_bool_arg, append_call_number_arg, append_call_string_arg, string_list,
};
use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};

use crate::mcp::context::McpContext;
use crate::presentation::TemplateOutput;
use crate::presentation::relationships as relationship_presentation;

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
					"description": "Explicit bound for listed neighbors and members. Otherwise the volume profile selects 40, 120, or 500 items; incomplete coverage produces a continuation."
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

	fn call(
		&self,
		context: &McpContext,
		arguments: &Value,
		output: OutputOptions,
	) -> Result<ToolResult, ToolError> {
		let request =
			graph_request(arguments, output.agent_options()).map_err(ToolError::failed)?;
		run_graph(context, request)
			.map(ToolResult::templated)
			.map_err(ToolError::failed)
	}
}

struct GraphRequest {
	focus: String,
	max_items: usize,
	direction: UsageDirection,
	relation: Vec<String>,
	min_count: usize,
	include_internal: bool,
	output: AgentOutputOptions,
}

fn graph_request(arguments: &Value, output: AgentOutputOptions) -> anyhow::Result<GraphRequest> {
	let focus = arguments
		.get("focus")
		.and_then(Value::as_str)
		.ok_or_else(|| anyhow::anyhow!("focus is required"))?;
	let requested_max_items = optional_u64(arguments, "max_items")?
		.map(|value| value as usize)
		.unwrap_or_else(|| graph_volume_limit(output.budget));
	if !(1..=MAX_BOUNDED_RESULT_ITEMS).contains(&requested_max_items) {
		anyhow::bail!("max_items must be between 1 and {MAX_BOUNDED_RESULT_ITEMS}");
	}
	let max_items = requested_max_items.min(graph_volume_limit(output.budget));
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
		output,
	})
}

fn graph_volume_limit(budget: OutputBudget) -> usize {
	match budget {
		OutputBudget::Small => GraphTool::DEFAULT_MAX_ITEMS,
		OutputBudget::Medium => 120,
		OutputBudget::Full => MAX_BOUNDED_RESULT_ITEMS,
	}
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

fn run_graph(context: &McpContext, request: GraphRequest) -> anyhow::Result<TemplateOutput> {
	let GraphRequest {
		focus: requested_focus,
		max_items,
		direction,
		relation,
		min_count,
		include_internal,
		output,
	} = request;
	let response = context.query_refreshed(
		Query::SymbolGraph(SymbolGraphQuery {
			workspace: None,
			focus: requested_focus.clone(),
			direction,
			relation: relation.clone(),
			min_count,
			include_internal,
			limit: max_items,
		}),
		Page::default(),
	)?;
	let QueryResult::SymbolGraph(result) = response.result else {
		anyhow::bail!("unexpected symbol graph response");
	};
	let next_call = graph_next_call(
		GraphNextInput {
			focus: &requested_focus,
			max_items,
			direction,
			relation: &relation,
			min_count,
			include_internal,
		},
		output,
		&result,
	);
	let view = GraphView {
		volume: output.budget.as_str(),
		direction: direction.as_str(),
		max_items,
		focus: &result.focus,
		coverage: &result.coverage,
		unlinked: &result.unlinked,
		show_callers: direction != UsageDirection::Outgoing,
		show_callees: direction != UsageDirection::Incoming,
		callers: &result.callers,
		callees: &result.callees,
		next_call,
	};
	relationship_presentation::graph(&view)
}

#[derive(Serialize)]
struct GraphView<'a> {
	volume: &'static str,
	direction: &'static str,
	max_items: usize,
	focus: &'a code_moniker_query::SymbolGraphFocus,
	coverage: &'a code_moniker_query::SymbolGraphCoverage,
	unlinked: &'a code_moniker_query::UnlinkedRefsDto,
	show_callers: bool,
	show_callees: bool,
	callers: &'a [code_moniker_query::SymbolGraphNeighbor],
	callees: &'a [code_moniker_query::SymbolGraphNeighbor],
	next_call: Option<GraphNextCall>,
}

#[derive(Serialize)]
struct GraphNextCall {
	focus: String,
	arguments: String,
}

struct GraphNextInput<'a> {
	focus: &'a str,
	max_items: usize,
	direction: UsageDirection,
	relation: &'a [String],
	min_count: usize,
	include_internal: bool,
}

fn graph_next_call(
	input: GraphNextInput<'_>,
	output: AgentOutputOptions,
	result: &SymbolGraphResult,
) -> Option<GraphNextCall> {
	let incomplete = result.coverage.members.returned < result.coverage.members.matching
		|| result.coverage.internal_edges.returned < result.coverage.internal_edges.matching
		|| result.coverage.callers.returned < result.coverage.callers.matching
		|| result.coverage.callees.returned < result.coverage.callees.matching;
	if !incomplete || input.max_items >= MAX_BOUNDED_RESULT_ITEMS {
		return None;
	}
	let next_budget = match output.budget {
		OutputBudget::Small => OutputBudget::Medium,
		OutputBudget::Medium | OutputBudget::Full => OutputBudget::Full,
	};
	let next_limit = input
		.max_items
		.saturating_mul(2)
		.max(graph_volume_limit(next_budget))
		.min(MAX_BOUNDED_RESULT_ITEMS);
	let mut arguments = String::new();
	append_call_number_arg(&mut arguments, "max_items", next_limit);
	append_call_string_arg(&mut arguments, "direction", input.direction.as_str());
	for relation in input.relation {
		append_call_string_arg(&mut arguments, "relation", relation);
	}
	append_call_number_arg(&mut arguments, "min_count", input.min_count);
	append_call_bool_arg(&mut arguments, "include_internal", input.include_internal);
	append_call_bool_arg(&mut arguments, "compact", output.compact);
	append_call_string_arg(&mut arguments, "budget", next_budget.as_str());
	Some(GraphNextCall {
		focus: input.focus.to_string(),
		arguments,
	})
}

#[cfg(test)]
mod tests {
	use code_moniker_query::{
		GraphSectionCoverage, SymbolGraphCoverage, SymbolGraphFocus, SymbolGraphNeighbor,
		UnlinkedRefsDto,
	};

	use super::*;
	use crate::presentation::RenderOptions;

	fn graph_markdown(
		result: &SymbolGraphResult,
		max_items: usize,
		direction: UsageDirection,
	) -> String {
		let view = GraphView {
			volume: "small",
			direction: direction.as_str(),
			max_items,
			focus: &result.focus,
			coverage: &result.coverage,
			unlinked: &result.unlinked,
			show_callers: direction != UsageDirection::Outgoing,
			show_callees: direction != UsageDirection::Incoming,
			callers: &result.callers,
			callees: &result.callees,
			next_call: None,
		};
		let rendered = relationship_presentation::graph(&view)
			.expect("graph template")
			.render(RenderOptions {
				compact: false,
				scheme: "code+moniker://",
				runtime: None,
			})
			.expect("render graph");
		crate::presentation::tests::validate_agent_markdown(&rendered, "Symbol graph", false)
			.expect("valid graph Markdown");
		rendered
	}

	#[test]
	fn graph_template_classifies_non_unique_references_outside_the_graph() {
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

		let rendered = graph_markdown(&result, 10, UsageDirection::Both);

		assert!(rendered.contains("- external: 1 (SDK 1, dependency 0, injected 0, unknown 0)"));
		assert!(rendered.contains("- candidate: 2"));
		assert!(rendered.contains("- dynamic: 3"));
		assert!(rendered.contains("- manifest-blocked: 4"));
		assert!(rendered.contains("- unresolved: 5"));
	}

	#[test]
	fn graph_template_omits_sections_filtered_by_direction() {
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

		let rendered = graph_markdown(&result, 10, UsageDirection::Outgoing);

		assert!(
			!rendered.contains("## Callers"),
			"a caller section cleared by direction=outgoing must not render as a fact, got:\n{rendered}"
		);
		assert!(
			rendered.contains("## Callees"),
			"the requested direction must still render, got:\n{rendered}"
		);
	}

	#[test]
	fn graph_template_labels_zero_after_filter_with_the_prefilter_total() {
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

		let rendered = graph_markdown(&result, 10, UsageDirection::Incoming);

		assert!(
			rendered.contains("- callers: 0/0 matching (2192 total)"),
			"a post-filter zero must retain its pre-filter denominator:\n{rendered}"
		);
	}

	#[test]
	fn graph_template_keeps_canonical_neighbor_uris() {
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

		let rendered = graph_markdown(&result, 10, UsageDirection::Incoming);

		assert!(
			rendered.contains(&format!("- uri: `{neighbor_uri}`")),
			"{rendered}"
		);
		assert!(
			!rendered.contains(&format!("- uri: `{local_uri}`")),
			"{rendered}"
		);
	}

	#[test]
	fn graph_volume_profiles_bound_the_query_before_rendering() {
		assert_eq!(graph_volume_limit(OutputBudget::Small), 40);
		assert_eq!(graph_volume_limit(OutputBudget::Medium), 120);
		assert_eq!(
			graph_volume_limit(OutputBudget::Full),
			MAX_BOUNDED_RESULT_ITEMS
		);
		let request = super::graph_request(
			&serde_json::json!({"focus": "rs:App", "max_items": 500}),
			AgentOutputOptions {
				compact: true,
				budget: OutputBudget::Small,
			},
		)
		.expect("small graph request");
		assert_eq!(request.max_items, GraphTool::DEFAULT_MAX_ITEMS);
	}
}
