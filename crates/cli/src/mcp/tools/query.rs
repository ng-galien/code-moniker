use code_moniker_query::{
	Query, QueryCursor, WorkspaceGeneration, parse_query, query_capability_spec, query_projection,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use code_moniker_workspace::code::compact_identity;

use super::common::{AgentOutputOptions, OutputBudget};
use super::scope::{
	append_call_bool_arg, append_call_cursor_arg, append_call_string_arg, cursor_argument,
};
use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;
use crate::presentation::query as query_presentation;

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
		"their dedicated MCP tool. Output is compact with a small result-volume profile by default."
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

	fn call(
		&self,
		context: &McpContext,
		arguments: &Value,
		output: OutputOptions,
	) -> Result<ToolResult, ToolError> {
		execute_query(context, arguments, output.agent_options())
	}
}

fn execute_query(
	context: &McpContext,
	arguments: &Value,
	output: AgentOutputOptions,
) -> Result<ToolResult, ToolError> {
	let expressions = query_expressions(arguments).map_err(ToolError::failed)?;
	let cursor = cursor_argument(arguments, "cursor")
		.map_err(ToolError::failed)?
		.map(|(offset, generation)| QueryCursor::new(offset, generation));
	if cursor.is_some() && expressions.len() != 1 {
		return Err(ToolError::failed(anyhow::anyhow!(
			"`cursor` can only resume a single `query`, not a `queries` batch"
		)));
	}
	let mut results = Vec::with_capacity(expressions.len());
	let mut generation = None;
	let mut cursor_partial = false;
	let mut volume_partial = false;
	let mut errors = 0usize;
	for (index, expression) in expressions.iter().enumerate() {
		match run_expression(
			context,
			expression,
			cursor.as_ref(),
			output,
			&mut generation,
		) {
			Ok((capability, body, next_cursor, volume_limited)) => {
				cursor_partial |= next_cursor.is_some();
				volume_partial |= volume_limited;
				let next_call = next_cursor
					.as_ref()
					.map(|cursor| query_next_call(expression, cursor, output))
					.or_else(|| {
						volume_limited
							.then(|| query_volume_next_call(expression, output))
							.flatten()
					});
				results.push(McpQueryResultView {
					number: (expressions.len() > 1).then_some(index + 1),
					operation: Some(capability),
					body: Some(body),
					error: None,
					next_call,
				});
			}
			Err(error) if expressions.len() == 1 => return Err(ToolError::failed(error)),
			Err(error) => {
				errors += 1;
				results.push(McpQueryResultView {
					number: Some(index + 1),
					operation: None,
					body: None,
					error: Some(format!("{error:#}")),
					next_call: None,
				});
			}
		}
	}
	let completeness = if errors > 0 {
		format!("partial ({errors} error(s))")
	} else if cursor_partial && volume_partial {
		"partial (cursor and volume profile)".to_string()
	} else if cursor_partial {
		"partial (cursor available)".to_string()
	} else if volume_partial {
		"partial (volume profile)".to_string()
	} else {
		"full".to_string()
	};
	let operation = if expressions.len() == 1 {
		"query"
	} else {
		"query.batch"
	};
	let view = McpQueryView {
		uri: "code+moniker://workspace",
		completeness,
		mode: operation,
		volume: output.budget.as_str(),
		results,
	};
	query_presentation::mcp(&view)
		.map(ToolResult::templated)
		.map_err(ToolError::failed)
}

#[derive(Serialize)]
struct McpQueryView {
	uri: &'static str,
	completeness: String,
	mode: &'static str,
	volume: &'static str,
	results: Vec<McpQueryResultView>,
}

#[derive(Serialize)]
struct McpQueryResultView {
	number: Option<usize>,
	operation: Option<&'static str>,
	body: Option<Value>,
	error: Option<String>,
	next_call: Option<String>,
}

fn run_expression(
	context: &McpContext,
	expression: &str,
	cursor: Option<&QueryCursor>,
	output: AgentOutputOptions,
	generation: &mut Option<WorkspaceGeneration>,
) -> anyhow::Result<(&'static str, Value, Option<QueryCursor>, bool)> {
	let mut request = parse_query(expression)?;
	let capability = request.query.capability();
	let projection = query_projection(&request.query).to_vec();
	let spec = query_capability_spec(capability)
		.ok_or_else(|| anyhow::anyhow!("query `{capability}` is not registered"))?;
	if !spec.read_only {
		anyhow::bail!("query `{capability}` is not declared read-only; use its dedicated MCP tool");
	}
	if spec.paginated {
		request.page.limit = request.page.limit.min(output.default_page_limit());
	}
	apply_query_volume_profile(&mut request.query, output);
	if let Some(cursor) = cursor {
		request.page.cursor = Some(cursor.clone());
	}
	let response = context.query(request)?;
	if let Some(observed) = response.generation {
		ensure_generation(generation, observed)?;
	}
	let next_cursor = response.next_cursor.clone();
	let mut body = serde_json::to_value(&response)?;
	apply_query_projection(&mut body, &projection);
	let volume_limited =
		apply_query_result_volume_profile(capability, &mut body, output.default_page_limit());
	if output.compact {
		compact_query_identity_fields(&mut body, context.scheme());
	}
	Ok((capability, body, next_cursor, volume_limited))
}

fn apply_query_volume_profile(query: &mut Query, output: AgentOutputOptions) {
	let max_items = output.default_page_limit();
	match query {
		Query::TreeChildren(query) => {
			query.depth = query.depth.min(query_tree_depth_limit(output.budget));
		}
		Query::SymbolSearch(query) | Query::SymbolInsights(query) => {
			query.context_lines = query
				.context_lines
				.min(query_context_lines_limit(output.budget));
			if output.budget == OutputBudget::Small {
				query.include_code = false;
			}
		}
		Query::SymbolDetail(query) => {
			query.context_lines = query
				.context_lines
				.min(query_context_lines_limit(output.budget));
		}
		Query::SyntaxTree(query) => query.max_nodes = query.max_nodes.min(max_items),
		Query::SyntaxParse(query) => query.max_nodes = query.max_nodes.min(max_items),
		Query::ViewRead(query) => {
			query.context_lines = query
				.context_lines
				.min(query_context_lines_limit(output.budget));
			if output.budget == OutputBudget::Small {
				query.include_code = false;
			}
		}
		Query::ChangeContext(query) => query.max_items = query.max_items.min(max_items),
		Query::SymbolGraph(query) => query.limit = query.limit.min(max_items),
		Query::GraphPath(query) => {
			query.max_symbols = query.max_symbols.min(max_items);
			query.max_edges = query.max_edges.min(max_items);
		}
		Query::GraphCorridor(query) => {
			query.max_symbols = query.max_symbols.min(max_items);
			query.max_edges = query.max_edges.min(max_items);
		}
		Query::IdentityChildren(query) => query.limit = query.limit.min(max_items),
		Query::ResolutionAudit(query) => query.limit = query.limit.min(max_items),
		Query::QueryDescribe(_)
		| Query::WorkspaceStatus
		| Query::SymbolUsages(_)
		| Query::RulesList(_)
		| Query::RulesCheck(_)
		| Query::RulesApplicable(_)
		| Query::ChangeReview(_)
		| Query::DiffImpactCompare(_)
		| Query::IdentityGraph(_)
		| Query::MetricsCoupling(_)
		| Query::Notes(_) => {}
	}
}

fn query_tree_depth_limit(budget: OutputBudget) -> usize {
	match budget {
		OutputBudget::Small => 2,
		OutputBudget::Medium => 8,
		OutputBudget::Full => 20,
	}
}

fn query_context_lines_limit(budget: OutputBudget) -> usize {
	match budget {
		OutputBudget::Small => 2,
		OutputBudget::Medium => 8,
		OutputBudget::Full => 20,
	}
}

fn apply_query_result_volume_profile(
	capability: &str,
	value: &mut Value,
	max_items: usize,
) -> bool {
	match capability {
		"change.review" => apply_change_review_volume_profile(value, max_items),
		_ => false,
	}
}

fn apply_change_review_volume_profile(value: &mut Value, max_items: usize) -> bool {
	let Some(Value::Object(review)) = value.pointer_mut("/result/data") else {
		return false;
	};
	let mut omitted = Map::new();
	for field in ["files", "symbol_changes", "ref_changes", "diagnostics"] {
		let Some(Value::Array(rows)) = review.get_mut(field) else {
			continue;
		};
		let omitted_count = rows.len().saturating_sub(max_items);
		rows.truncate(max_items);
		if omitted_count > 0 {
			omitted.insert(field.to_string(), json!(omitted_count));
		}
	}
	if omitted.is_empty() {
		return false;
	}
	review.insert(
		"volume_projection".to_string(),
		json!({
			"limit_per_collection": max_items,
			"omitted": omitted,
		}),
	);
	true
}

fn query_next_call(
	expression: &str,
	next_cursor: &QueryCursor,
	output: AgentOutputOptions,
) -> String {
	let mut arguments = String::new();
	append_call_string_arg(&mut arguments, "query", expression);
	append_call_cursor_arg(&mut arguments, "cursor", next_cursor);
	if output.budget != OutputBudget::Small {
		append_call_string_arg(&mut arguments, "budget", output.budget.as_str());
	}
	if !output.compact {
		append_call_bool_arg(&mut arguments, "compact", false);
	}
	arguments
}

fn query_volume_next_call(expression: &str, output: AgentOutputOptions) -> Option<String> {
	let next_budget = match output.budget {
		OutputBudget::Small => OutputBudget::Medium,
		OutputBudget::Medium => OutputBudget::Full,
		OutputBudget::Full => return None,
	};
	let mut arguments = String::new();
	append_call_string_arg(&mut arguments, "query", expression);
	append_call_string_arg(&mut arguments, "budget", next_budget.as_str());
	if !output.compact {
		append_call_bool_arg(&mut arguments, "compact", false);
	}
	Some(arguments)
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

fn apply_query_projection(value: &mut Value, projection: &[String]) {
	if projection.is_empty() {
		return;
	}
	let Some(data) = value.pointer_mut("/result/data") else {
		return;
	};
	if let Some(rows) = data.get_mut("rows").and_then(Value::as_array_mut) {
		for row in rows {
			project_object(row, projection);
		}
	} else {
		project_object(data, projection);
	}
}

fn project_object(value: &mut Value, projection: &[String]) {
	let Value::Object(object) = value else {
		return;
	};
	let mut projected = Map::new();
	for field in projection {
		if let Some(value) = object.remove(field) {
			projected.insert(field.clone(), value);
		}
	}
	*object = projected;
}

fn compact_query_identity_fields(value: &mut Value, scheme: &str) {
	compact_query_identity_fields_inner(value, scheme, false);
}

fn compact_query_identity_fields_inner(value: &mut Value, scheme: &str, preserve_evidence: bool) {
	let Value::Object(fields) = value else {
		if let Value::Array(values) = value {
			for value in values {
				compact_query_identity_fields_inner(value, scheme, preserve_evidence);
			}
		}
		return;
	};
	let identity_edge = (fields.contains_key("kinds") && fields.contains_key("count"))
		|| (fields.contains_key("relation") && fields.contains_key("reference"));
	for (field, value) in fields {
		if preserve_evidence && matches!(field.as_str(), "source" | "snippet" | "evidence") {
			continue;
		}
		let identity_field =
			matches!(
				field.as_str(),
				"uri"
					| "moniker" | "identity"
					| "focus" | "prefix"
					| "from" | "to" | "old_uri"
					| "new_uri" | "old_target"
					| "new_target" | "reference"
					| "endpoint" | "via"
					| "candidates"
			) || (identity_edge && matches!(field.as_str(), "source" | "target"));
		if identity_field {
			compact_query_identity_value(value, scheme);
		} else {
			compact_query_identity_fields_inner(value, scheme, preserve_evidence);
		}
	}
}

fn compact_query_identity_value(value: &mut Value, scheme: &str) {
	match value {
		Value::String(identity) if identity.starts_with(scheme) => {
			if let Some(compact) = compact_identity(identity, scheme) {
				*identity = compact;
			}
		}
		Value::Array(values) => {
			for value in values {
				compact_query_identity_value(value, scheme);
			}
		}
		Value::Object(_) => compact_query_identity_fields_inner(value, scheme, true),
		_ => {}
	}
}

#[cfg(test)]
mod tests {
	use code_moniker_query::{Query, QueryCursor, parse_query};
	use serde_json::json;

	use super::{AgentOutputOptions, McpQueryResultView, McpQueryView, OutputBudget};

	fn output(budget: OutputBudget) -> AgentOutputOptions {
		AgentOutputOptions {
			compact: true,
			budget,
		}
	}

	fn profiled_query(expression: &str, budget: OutputBudget) -> Query {
		let mut request = parse_query(expression).expect("query expression");
		super::apply_query_volume_profile(&mut request.query, output(budget));
		request.query
	}

	#[test]
	fn query_next_call_preserves_the_volume_profile() {
		let output = super::AgentOutputOptions::from_arguments(&serde_json::json!({
			"budget": "medium"
		}))
		.expect("output options");
		let call = super::query_next_call(
			"symbol.search name:\"App\"",
			&QueryCursor::new(20, None),
			output,
		);

		assert!(call.contains("budget=\"medium\""), "{call}");
		assert!(call.contains("cursor=20"), "{call}");
	}

	#[test]
	fn query_next_call_json_encodes_multiline_expressions_for_replay_and_commonmark() {
		let expression = concat!(
			"symbol.search name:\"App \\\"Service\\\" \\\\ ",
			"\u{0008} handler\"\r\n\tproject name uri"
		);
		parse_query(expression).expect("original query");
		let call = super::query_next_call(
			expression,
			&QueryCursor::new(20, None),
			output(OutputBudget::Medium),
		);

		assert!(!call.chars().any(char::is_control), "{call:?}");
		let encoded = call.strip_prefix(" query=").expect("query argument");
		let mut strings = serde_json::Deserializer::from_str(encoded).into_iter::<String>();
		let replayed = strings.next().expect("encoded query").expect("JSON string");
		let suffix = &encoded[strings.byte_offset()..];
		assert_eq!(replayed, expression);
		assert_eq!(suffix, " cursor=20 budget=\"medium\"");
		parse_query(&replayed).expect("replayed query");

		let view = McpQueryView {
			uri: "code+moniker://workspace",
			completeness: "partial (cursor available)".to_string(),
			mode: "query",
			volume: "medium",
			results: vec![McpQueryResultView {
				number: None,
				operation: Some("symbol.search"),
				body: Some(json!({"rows": []})),
				error: None,
				next_call: Some(call.clone()),
			}],
		};
		let rendered = crate::presentation::query::mcp(&view)
			.expect("query template")
			.render(crate::presentation::RenderOptions {
				compact: true,
				scheme: "code+moniker://",
				runtime: None,
			})
			.expect("rendered query");
		crate::presentation::tests::validate_agent_markdown(&rendered, "Query results", false)
			.expect("query CommonMark");
		assert!(rendered.contains(&format!("code_moniker_query{call}")));
	}

	#[test]
	fn change_review_volume_follow_up_increases_profile_and_preserves_compact_mode() {
		let expression = "change.review\nconsistency stale-ok";
		let medium = super::query_volume_next_call(
			expression,
			AgentOutputOptions {
				compact: false,
				budget: OutputBudget::Small,
			},
		)
		.expect("medium follow-up");
		let encoded = medium.strip_prefix(" query=").expect("query argument");
		let mut strings = serde_json::Deserializer::from_str(encoded).into_iter::<String>();
		assert_eq!(
			strings.next().expect("encoded query").expect("JSON string"),
			expression
		);
		assert_eq!(
			&encoded[strings.byte_offset()..],
			" budget=\"medium\" compact=false"
		);

		let full = super::query_volume_next_call(expression, output(OutputBudget::Medium))
			.expect("full follow-up");
		assert!(full.ends_with(" budget=\"full\""), "{full}");
		assert!(super::query_volume_next_call(expression, output(OutputBudget::Full)).is_none());
	}

	#[test]
	fn non_paginated_query_volume_profiles_cap_all_explicit_item_counts() {
		for (budget, expected) in [
			(OutputBudget::Small, 20),
			(OutputBudget::Medium, 80),
			(OutputBudget::Full, 500),
		] {
			for expression in [
				"syntax.tree focus:\"src/lib.rs\" max_nodes:20000",
				"syntax.parse language:\"rs\" source:\"fn main() {}\" max_nodes:20000",
			] {
				let query = profiled_query(expression, budget);
				let max_nodes = match query {
					Query::SyntaxTree(query) => query.max_nodes,
					Query::SyntaxParse(query) => query.max_nodes,
					_ => panic!("expected syntax query"),
				};
				assert_eq!(max_nodes, expected, "{budget:?} {expression}");
			}

			let Query::SymbolGraph(query) =
				profiled_query("symbol.graph focus:\"rs:App\" max_items:500", budget)
			else {
				panic!("symbol.graph query");
			};
			assert_eq!(query.limit, expected, "{budget:?} symbol.graph");

			let Query::IdentityChildren(query) =
				profiled_query("identity.children prefix:\"lang:rs\" max_items:500", budget)
			else {
				panic!("identity.children query");
			};
			assert_eq!(query.limit, expected, "{budget:?} identity.children");

			let Query::ChangeContext(query) =
				profiled_query("change.context focus:\"rs:App\" max_items:100", budget)
			else {
				panic!("change.context query");
			};
			assert_eq!(
				query.max_items,
				expected.min(100),
				"{budget:?} change.context"
			);
		}
	}

	#[test]
	fn context_profiles_cap_symbol_detail_and_small_omits_view_code_before_execution() {
		for (budget, expected) in [
			(OutputBudget::Small, 2),
			(OutputBudget::Medium, 8),
			(OutputBudget::Full, 20),
		] {
			let Query::SymbolDetail(query) =
				profiled_query("symbol.detail uri:\"rs:App\" context_lines:1000", budget)
			else {
				panic!("symbol.detail query");
			};
			assert_eq!(query.context_lines, expected, "{budget:?}");
		}

		let Query::ViewRead(query) = profiled_query(
			"view.read uri:\"workspace/views/app\" context_lines:1000 include_code:true",
			OutputBudget::Small,
		) else {
			panic!("view.read query");
		};
		assert_eq!(query.context_lines, 2);
		assert!(!query.include_code);
	}

	#[test]
	fn small_profile_caps_paginated_internal_volume_before_execution() {
		let Query::TreeChildren(query) = profiled_query(
			"tree.children path:\"src/**\" depth:100 limit:500",
			OutputBudget::Small,
		) else {
			panic!("tree.children query");
		};
		assert_eq!(query.depth, 2);

		let Query::SymbolSearch(query) = profiled_query(
			concat!(
				"symbol.search name:\"App\" include_code:true ",
				"context_lines:1000 limit:500"
			),
			OutputBudget::Small,
		) else {
			panic!("symbol.search query");
		};
		assert_eq!(query.context_lines, 2);
		assert!(!query.include_code);

		let Query::ResolutionAudit(query) = profiled_query(
			"resolution.audit prefix:\"lang:rs\" limit:200",
			OutputBudget::Small,
		) else {
			panic!("resolution.audit query");
		};
		assert_eq!(query.limit, 20);
	}

	#[test]
	fn small_profile_structurally_bounds_change_review_before_rendering() {
		let rows = (0..25)
			.map(|index| json!({"index": index}))
			.collect::<Vec<_>>();
		let mut body = json!({
			"result": {
				"kind": "change_review",
				"data": {
					"summary": {"files": 25, "symbol_changes": 25, "ref_changes": 25},
					"files": rows,
					"symbol_changes": rows,
					"ref_changes": rows,
					"diagnostics": rows
				}
			}
		});

		assert!(super::apply_change_review_volume_profile(&mut body, 20));
		for field in ["files", "symbol_changes", "ref_changes", "diagnostics"] {
			assert_eq!(
				body.pointer(&format!("/result/data/{field}"))
					.and_then(serde_json::Value::as_array)
					.map(Vec::len),
				Some(20),
				"{field}"
			);
		}
		assert_eq!(body.pointer("/result/data/summary/files"), Some(&json!(25)));
		for field in ["files", "symbol_changes", "ref_changes", "diagnostics"] {
			assert_eq!(
				body.pointer(&format!("/result/data/volume_projection/omitted/{field}")),
				Some(&json!(5)),
				"{field} omitted"
			);
		}
		assert_eq!(
			body.pointer("/result/data/volume_projection/limit_per_collection"),
			Some(&json!(20))
		);
	}

	#[test]
	fn query_describe_is_not_structurally_truncated_by_the_volume_profile() {
		let capabilities = (0..25)
			.map(|index| json!({"name": format!("query-{index}")}))
			.collect::<Vec<_>>();
		let mut body = json!({
			"result": {
				"kind": "query_describe",
				"data": {"capabilities": capabilities}
			}
		});

		assert!(!super::apply_query_result_volume_profile(
			"query.describe",
			&mut body,
			20
		));
		assert_eq!(
			body.pointer("/result/data/capabilities")
				.and_then(serde_json::Value::as_array)
				.map(Vec::len),
			Some(25)
		);
	}

	#[test]
	fn graph_path_and_corridor_volume_profiles_cap_explicit_traversal_counts() {
		for (budget, expected) in [
			(OutputBudget::Small, 20),
			(OutputBudget::Medium, 80),
			(OutputBudget::Full, 500),
		] {
			for expression in [
				concat!(
					"graph.path from:\"rs:App\" to:\"rs:Target\" ",
					"max_symbols:100000 max_edges:500000"
				),
				concat!(
					"graph.corridor from:\"rs:App\" to:\"rs:Target\" ",
					"relation:calls shape:callable max_symbols:100000 max_edges:500000"
				),
			] {
				let query = profiled_query(expression, budget);
				let (max_symbols, max_edges) = match query {
					Query::GraphPath(query) => (query.max_symbols, query.max_edges),
					Query::GraphCorridor(query) => (query.max_symbols, query.max_edges),
					_ => panic!("expected graph traversal query"),
				};
				assert_eq!(max_symbols, expected, "{budget:?} {expression}");
				assert_eq!(max_edges, expected, "{budget:?} {expression}");
			}
		}
	}

	#[test]
	fn query_compaction_preserves_source_literals_and_compacts_typed_identities() {
		let canonical = "code+moniker://./lang:rs/module:app/fn:run()";
		let compact = code_moniker_workspace::code::compact_identity(canonical, "code+moniker://")
			.expect("compact identity");
		let mut value = json!({
			"result": {
				"data": {
					"uri": canonical,
					"moniker": canonical,
					"identity": canonical,
					"candidates": [canonical, "plain"],
					"edge": {"source": canonical, "target": canonical, "kinds": ["calls"], "count": 1},
					"focus": {
						"uri": canonical,
						"source": canonical,
						"snippet": canonical,
						"evidence": canonical
					},
					"from": {"identity": canonical, "source": canonical},
					"to": [{"moniker": canonical, "evidence": canonical}],
					"sample": {
						"snippet": canonical,
						"source": canonical,
						"evidence": canonical,
						"nested": [{"source": canonical, "snippet": canonical}]
					}
				}
			}
		});

		super::compact_query_identity_fields(&mut value, "code+moniker://");

		assert_eq!(value.pointer("/result/data/uri"), Some(&json!(compact)));
		assert_eq!(value.pointer("/result/data/moniker"), Some(&json!(compact)));
		assert_eq!(
			value.pointer("/result/data/candidates/0"),
			Some(&json!(compact))
		);
		assert_eq!(
			value.pointer("/result/data/edge/source"),
			Some(&json!(compact))
		);
		for pointer in [
			"/result/data/focus/uri",
			"/result/data/from/identity",
			"/result/data/to/0/moniker",
		] {
			assert_eq!(value.pointer(pointer), Some(&json!(compact)), "{pointer}");
		}
		for pointer in [
			"/result/data/focus/source",
			"/result/data/focus/snippet",
			"/result/data/focus/evidence",
			"/result/data/from/source",
			"/result/data/to/0/evidence",
			"/result/data/sample/snippet",
			"/result/data/sample/source",
			"/result/data/sample/evidence",
			"/result/data/sample/nested/0/source",
			"/result/data/sample/nested/0/snippet",
		] {
			assert_eq!(value.pointer(pointer), Some(&json!(canonical)), "{pointer}");
		}
	}
}
