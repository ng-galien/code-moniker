use code_moniker_query::{Query, QueryResult, SymbolUsagesQuery, SymbolUsagesResult, UsageDto};
use serde::Serialize;
use serde_json::{Value, json};

use super::common::{AgentOutputOptions, OutputBudget, is_workspace_uri};
use super::scope::{
	Paging, ScopeFilter, append_call_bool_arg, append_call_cursor_arg, append_call_number_arg,
	append_call_string_arg,
};
use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;
use crate::presentation::TemplateOutput;
use crate::presentation::relationships as relationship_presentation;

mod compact;

use compact::{CompactUsageMap, compact_usage_map};

const DEFAULT_MAX_EVIDENCE: usize = 4;
const MAX_EVIDENCE: usize = 12;
const DEFAULT_USAGE_CONTEXT_LINES: usize = 2;
const MAX_USAGE_CONTEXT_LINES: usize = 8;

pub(in crate::mcp) struct UsagesTool;

impl UsagesTool {
	pub(super) const NAME: &'static str = "code_moniker_usages";

	const DESCRIPTION: &'static str = concat!(
		"When to use: inspect who uses a symbol returned by code_moniker_symbols. ",
		"Use this to decide whether a module/type/function behaves like a shared helper or is only locally consumed.\n",
		"\n",
		"Read symbolic usage edges.\n",
		"  direction=incoming — consumers of the target symbol\n",
		"  direction=outgoing — dependencies used by the target symbol\n",
		"  direction=both     — both sections\n",
		"Set include_descendants=true to roll member activity into an owner such as a type without changing exact-symbol usage semantics.\n",
		"Incoming usage diagnostics include file, context, prefix concentration, reference kinds, and a shared-helper signal. ",
		"Compact output groups repeated references by symbolic context, summarizes technical noise, and includes a bounded set of representative source excerpts."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"uri": {
					"type": "string",
					"description": "Compact moniker, canonical URI, symbol id, unique bare name, or unambiguous lang:path.kind:name reference. Ambiguity returns candidates."
				},
				"direction": {
					"type": "string",
					"enum": ["incoming", "outgoing", "both"],
					"description": "Usage direction to render."
				},
				"include_descendants": {
					"type": "boolean",
					"default": false,
					"description": "Include navigable descendant members of the target and exclude relations internal to that owner boundary."
				},
				"evidence": {
					"type": "string",
					"enum": ["none", "representative"],
					"default": "representative",
					"description": "In compact mode, attach source excerpts to a bounded, direction-balanced selection of semantic usage groups."
				},
				"technical": {
					"type": "string",
					"enum": ["summary", "include"],
					"default": "summary",
					"description": "Summarize imports, annotations, and non-primary type relations by default; include lists their groups without source excerpts."
				},
				"max_evidence": {
					"type": "integer",
					"minimum": 0,
					"maximum": MAX_EVIDENCE,
					"default": DEFAULT_MAX_EVIDENCE,
					"description": "Explicit maximum representative source excerpts in compact mode. Otherwise the volume profile selects 4, 8, or 12 excerpts."
				},
				"context_lines": {
					"type": "integer",
					"minimum": 0,
					"maximum": MAX_USAGE_CONTEXT_LINES,
					"default": DEFAULT_USAGE_CONTEXT_LINES,
					"description": "Explicit source lines around each representative usage. Otherwise the volume profile selects 2, 4, or 8 lines."
				},
				"path": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Filter usage locations by relative file glob(s), OR-combined."
				},
				"lang": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Filter usage locations by language tag(s), OR-combined."
				},
				"limit": {
					"type": "integer",
					"minimum": 1,
					"maximum": super::scope::MAX_LIMIT,
					"description": "Target usage-reference page size. A page may extend past it to keep one symbolic group intact."
				},
				"cursor": {
					"oneOf": [{ "type": "integer" }, { "type": "string" }],
					"description": "Opaque row offset returned in next calls."
				}
			},
			"required": ["uri"],
			"additionalProperties": false
		})
	}
}

impl McpTool for UsagesTool {
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
		let request = usage_request_from_arguments(arguments, output.agent_options())
			.map_err(ToolError::failed)?;
		read_usages(context, &request)
			.map(ToolResult::templated)
			.map_err(ToolError::failed)
	}
}

struct UsageRequest {
	uri: String,
	direction: UsageDirection,
	include_descendants: bool,
	scope: ScopeFilter,
	paging: Paging,
	compact: bool,
	evidence: EvidenceMode,
	technical: TechnicalMode,
	max_evidence: usize,
	context_lines: usize,
	output: AgentOutputOptions,
}

fn usage_request_from_arguments(
	arguments: &Value,
	output: AgentOutputOptions,
) -> anyhow::Result<UsageRequest> {
	Ok(UsageRequest {
		uri: arguments
			.get("uri")
			.and_then(Value::as_str)
			.ok_or_else(|| anyhow::anyhow!("`uri` is required"))?
			.to_string(),
		direction: UsageDirection::from_arguments(arguments)?,
		include_descendants: arguments
			.get("include_descendants")
			.map(|value| {
				value
					.as_bool()
					.ok_or_else(|| anyhow::anyhow!("`include_descendants` must be a boolean"))
			})
			.transpose()?
			.unwrap_or(false),
		scope: ScopeFilter::from_arguments(arguments)?,
		paging: Paging::from_arguments_for_volume(arguments, output)?,
		compact: output.compact,
		evidence: EvidenceMode::from_arguments(arguments)?,
		technical: TechnicalMode::from_arguments(arguments)?,
		max_evidence: strict_bounded_usize_argument(
			arguments,
			"max_evidence",
			usage_evidence_limit(output.budget),
			MAX_EVIDENCE,
		)?
		.min(usage_evidence_limit(output.budget)),
		context_lines: strict_bounded_usize_argument(
			arguments,
			"context_lines",
			usage_context_lines(output.budget),
			MAX_USAGE_CONTEXT_LINES,
		)?
		.min(usage_context_lines(output.budget)),
		output,
	})
}

fn usage_evidence_limit(budget: OutputBudget) -> usize {
	match budget {
		OutputBudget::Small => DEFAULT_MAX_EVIDENCE,
		OutputBudget::Medium => 8,
		OutputBudget::Full => MAX_EVIDENCE,
	}
}

fn usage_context_lines(budget: OutputBudget) -> usize {
	match budget {
		OutputBudget::Small => DEFAULT_USAGE_CONTEXT_LINES,
		OutputBudget::Medium => 4,
		OutputBudget::Full => MAX_USAGE_CONTEXT_LINES,
	}
}

fn strict_bounded_usize_argument(
	arguments: &Value,
	name: &str,
	default: usize,
	maximum: usize,
) -> anyhow::Result<usize> {
	let Some(value) = arguments.get(name) else {
		return Ok(default);
	};
	let value = value
		.as_u64()
		.ok_or_else(|| anyhow::anyhow!("`{name}` must be an unsigned integer"))?;
	if value > maximum as u64 {
		anyhow::bail!("`{name}` must be at most {maximum}");
	}
	Ok(value as usize)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EvidenceMode {
	None,
	Representative,
}

impl EvidenceMode {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		let value = optional_string_argument(arguments, "evidence")?.unwrap_or("representative");
		match value {
			"none" => Ok(Self::None),
			"representative" => Ok(Self::Representative),
			value => anyhow::bail!("unknown usage evidence mode `{value}`"),
		}
	}

	fn as_str(self) -> &'static str {
		match self {
			Self::None => "none",
			Self::Representative => "representative",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TechnicalMode {
	Summary,
	Include,
}

impl TechnicalMode {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		let value = optional_string_argument(arguments, "technical")?.unwrap_or("summary");
		match value {
			"summary" => Ok(Self::Summary),
			"include" => Ok(Self::Include),
			value => anyhow::bail!("unknown technical usage mode `{value}`"),
		}
	}

	fn as_str(self) -> &'static str {
		match self {
			Self::Summary => "summary",
			Self::Include => "include",
		}
	}
}

fn optional_string_argument<'a>(
	arguments: &'a Value,
	name: &str,
) -> anyhow::Result<Option<&'a str>> {
	match arguments.get(name) {
		Some(Value::String(value)) => Ok(Some(value)),
		Some(_) => anyhow::bail!("`{name}` must be a string"),
		None => Ok(None),
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mcp) enum UsageDirection {
	Incoming,
	Outgoing,
	Both,
}

impl UsageDirection {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		match arguments
			.get("direction")
			.and_then(Value::as_str)
			.unwrap_or("incoming")
		{
			"incoming" => Ok(Self::Incoming),
			"outgoing" => Ok(Self::Outgoing),
			"both" => Ok(Self::Both),
			value => anyhow::bail!("unknown usage direction `{value}`"),
		}
	}

	fn as_str(self) -> &'static str {
		match self {
			Self::Incoming => "incoming",
			Self::Outgoing => "outgoing",
			Self::Both => "both",
		}
	}
}

fn read_usages(context: &McpContext, request: &UsageRequest) -> anyhow::Result<TemplateOutput> {
	if is_workspace_uri(&request.uri, context.scheme(), "workspace") {
		anyhow::bail!("usage reads require a symbol moniker returned by code_moniker_symbols");
	}
	let response = context.query_refreshed(
		Query::SymbolUsages(SymbolUsagesQuery {
			workspace: None,
			uri: request.uri.clone(),
			direction: match request.direction {
				UsageDirection::Incoming => code_moniker_query::UsageDirection::Incoming,
				UsageDirection::Outgoing => code_moniker_query::UsageDirection::Outgoing,
				UsageDirection::Both => code_moniker_query::UsageDirection::Both,
			},
			path: request.scope.paths.clone(),
			lang: request.scope.langs.clone(),
			include_descendants: request.include_descendants,
			projection: Vec::new(),
		}),
		request.paging.daemon_page(),
	)?;
	let QueryResult::SymbolUsages(result) = response.result else {
		anyhow::bail!("unexpected daemon response for usages");
	};
	usages_template(context, request, response.next_cursor.as_ref(), &result)
}

#[derive(Serialize)]
struct McpUsagesScope<'a> {
	paths: &'a [String],
	langs: &'a [String],
}

#[derive(Serialize)]
struct McpUsagesView<'a> {
	uri: &'a str,
	partial: bool,
	start: usize,
	end: usize,
	total: usize,
	next_cursor: Option<usize>,
	direction: &'static str,
	target_scope: &'static str,
	targets: usize,
	limit: usize,
	volume: &'static str,
	target: &'a code_moniker_query::SymbolDto,
	scope: McpUsagesScope<'a>,
	show_incoming: bool,
	show_outgoing: bool,
	incoming_summary: Option<&'a code_moniker_query::UsageSummaryDto>,
	outgoing_summary: Option<&'a code_moniker_query::UsageSummaryDto>,
	compact_map: Option<CompactUsageMap>,
	usages: Option<&'a [UsageDto]>,
	next_calls: Vec<McpUsageNextCall>,
}

#[derive(Serialize)]
struct McpUsageNextCall {
	tool: &'static str,
	uri: Option<String>,
	arguments: String,
}

fn usages_template(
	context: &McpContext,
	request: &UsageRequest,
	next_cursor: Option<&code_moniker_query::QueryCursor>,
	result: &SymbolUsagesResult,
) -> anyhow::Result<TemplateOutput> {
	let start = request.paging.cursor.min(result.total);
	let end = start.saturating_add(result.rows.len()).min(result.total);
	let incoming_summary = matches!(
		request.direction,
		UsageDirection::Incoming | UsageDirection::Both
	)
	.then_some(result.incoming_summary.as_ref())
	.flatten();
	let outgoing_summary = matches!(
		request.direction,
		UsageDirection::Outgoing | UsageDirection::Both
	)
	.then_some(result.outgoing_summary.as_ref())
	.flatten();
	let compact_map = request
		.compact
		.then(|| compact_usage_map(context, result, request));
	let usages = (!request.compact).then_some(result.rows.as_slice());
	let next_calls = usage_next_calls(context.scheme(), request, next_cursor, result);
	let view = McpUsagesView {
		uri: &result.target.uri,
		partial: next_cursor.is_some(),
		start,
		end,
		total: result.total,
		next_cursor: next_cursor.map(|cursor| cursor.offset),
		direction: request.direction.as_str(),
		target_scope: if result.include_descendants {
			"descendants"
		} else {
			"exact"
		},
		targets: result.targets,
		limit: request.paging.limit,
		volume: request.output.budget.as_str(),
		target: &result.target,
		scope: McpUsagesScope {
			paths: &request.scope.paths,
			langs: &request.scope.langs,
		},
		show_incoming: matches!(
			request.direction,
			UsageDirection::Incoming | UsageDirection::Both
		),
		show_outgoing: matches!(
			request.direction,
			UsageDirection::Outgoing | UsageDirection::Both
		),
		incoming_summary,
		outgoing_summary,
		compact_map,
		usages,
		next_calls,
	};
	let candidates = result
		.rows
		.iter()
		.filter_map(|row| row.via.as_deref())
		.filter_map(via_moniker);
	Ok(relationship_presentation::usages(&view)?.with_monikers(candidates))
}

fn usage_next_calls(
	scheme: &str,
	request: &UsageRequest,
	next_cursor: Option<&code_moniker_query::QueryCursor>,
	result: &SymbolUsagesResult,
) -> Vec<McpUsageNextCall> {
	let mut calls = Vec::new();
	if let Some(next) = next_cursor {
		calls.push(usages_call(UsageCallArguments {
			target_uri: &result.target.uri,
			direction: request.direction,
			scope: &request.scope,
			limit: request.paging.limit,
			cursor: Some(next),
			evidence: request.evidence,
			technical: request.technical,
			max_evidence: request.max_evidence,
			context_lines: request.context_lines,
			include_descendants: request.include_descendants,
			output: request.output,
		}));
	}
	calls.push(read_call(&result.target.uri, request.output));
	if matches!(
		request.direction,
		UsageDirection::Incoming | UsageDirection::Both
	) && let Some(context) = result
		.rows
		.iter()
		.filter(|row| row.direction == code_moniker_query::UsageDirection::Incoming)
		.map(|row| row.context.as_str())
		.find(|context| context.starts_with(scheme) && *context != result.target.uri)
	{
		calls.push(read_call(context, request.output));
	}
	if !request.compact {
		calls.push(usages_call(UsageCallArguments {
			target_uri: &result.target.uri,
			direction: UsageDirection::Incoming,
			scope: &request.scope,
			limit: 50,
			cursor: None,
			evidence: request.evidence,
			technical: request.technical,
			max_evidence: request.max_evidence,
			context_lines: request.context_lines,
			include_descendants: request.include_descendants,
			output: request.output,
		}));
		calls.push(usages_call(UsageCallArguments {
			target_uri: &result.target.uri,
			direction: UsageDirection::Outgoing,
			scope: &request.scope,
			limit: 50,
			cursor: None,
			evidence: request.evidence,
			technical: request.technical,
			max_evidence: request.max_evidence,
			context_lines: request.context_lines,
			include_descendants: request.include_descendants,
			output: request.output,
		}));
		let mut arguments = String::new();
		request.scope.append_call_args(&mut arguments);
		append_call_string_arg(&mut arguments, "name", &result.target.name);
		append_call_number_arg(&mut arguments, "limit", 20);
		append_output_args(&mut arguments, request.output);
		calls.push(McpUsageNextCall {
			tool: "code_moniker_symbols",
			uri: Some(format!("{scheme}workspace")),
			arguments,
		});
	}
	calls
}

fn via_moniker(via: &str) -> Option<&str> {
	let start = via.rfind("(code+moniker://")?.saturating_add(1);
	via.get(start..via.len().checked_sub(1)?)
}

struct UsageCallArguments<'a> {
	target_uri: &'a str,
	direction: UsageDirection,
	scope: &'a ScopeFilter,
	limit: usize,
	cursor: Option<&'a code_moniker_query::QueryCursor>,
	evidence: EvidenceMode,
	technical: TechnicalMode,
	max_evidence: usize,
	context_lines: usize,
	include_descendants: bool,
	output: AgentOutputOptions,
}

fn usages_call(call: UsageCallArguments<'_>) -> McpUsageNextCall {
	let mut arguments = String::new();
	append_call_string_arg(&mut arguments, "direction", call.direction.as_str());
	if call.include_descendants {
		append_call_bool_arg(&mut arguments, "include_descendants", true);
	}
	call.scope.append_call_args(&mut arguments);
	append_call_number_arg(&mut arguments, "limit", call.limit);
	if let Some(cursor) = call.cursor {
		append_call_cursor_arg(&mut arguments, "cursor", cursor);
	}
	if call.evidence != EvidenceMode::Representative {
		append_call_string_arg(&mut arguments, "evidence", call.evidence.as_str());
	}
	if call.technical != TechnicalMode::Summary {
		append_call_string_arg(&mut arguments, "technical", call.technical.as_str());
	}
	if call.max_evidence != DEFAULT_MAX_EVIDENCE {
		append_call_number_arg(&mut arguments, "max_evidence", call.max_evidence);
	}
	if call.context_lines != DEFAULT_USAGE_CONTEXT_LINES {
		append_call_number_arg(&mut arguments, "context_lines", call.context_lines);
	}
	append_output_args(&mut arguments, call.output);
	McpUsageNextCall {
		tool: "code_moniker_usages",
		uri: Some(call.target_uri.to_string()),
		arguments,
	}
}

fn read_call(uri: &str, output: AgentOutputOptions) -> McpUsageNextCall {
	let mut arguments = String::new();
	append_call_number_arg(&mut arguments, "context_lines", 3);
	append_output_args(&mut arguments, output);
	McpUsageNextCall {
		tool: "code_moniker_read",
		uri: Some(uri.to_string()),
		arguments,
	}
}

fn append_output_args(arguments: &mut String, output: AgentOutputOptions) {
	append_call_bool_arg(arguments, "compact", output.compact);
	append_call_string_arg(arguments, "budget", output.budget.as_str());
}

fn usage_kind_priority(kind: &str) -> u8 {
	match kind {
		"implements" | "extends" | "inherits" => 0,
		"method_call" | "calls" => 10,
		"constructs" | "instantiates" => 20,
		"reads" | "uses_type" | "returns_type" | "annotates" => 30,
		"imports" | "imports_symbol" | "imports_module" => 40,
		kind if kind.starts_with("imports_") => 40,
		_ => 50,
	}
}

#[cfg(test)]
mod tests {
	use code_moniker_query::{QueryCursor, SymbolDto, SymbolUsagesResult, UsageDto};
	use serde_json::json;

	use super::{AgentOutputOptions, OutputBudget, usage_next_calls, usage_request_from_arguments};

	#[test]
	fn daemon_usage_next_follows_the_first_navigable_context() {
		let scheme = "acme://";
		let target_uri = "acme://./module:model/fn:refresh_plan_from_event(event)";
		let wrapper_uri = "acme://./module:model/struct:Plan/method:from_event(event)";
		let output = AgentOutputOptions {
			compact: true,
			budget: OutputBudget::Small,
		};
		let request = usage_request_from_arguments(
			&json!({
				"uri": target_uri,
				"direction": "incoming",
				"include_descendants": true
			}),
			output,
		)
		.expect("usage request");
		let result = SymbolUsagesResult {
			target: SymbolDto {
				root: "/workspace".to_string(),
				uri: target_uri.to_string(),
				id: "symbol:1:1".to_string(),
				name: "refresh_plan_from_event(event)".to_string(),
				kind: "fn".to_string(),
				visibility: "private".to_string(),
				signature: String::new(),
				file: "src/model.rs".to_string(),
				language: "rs".to_string(),
				line_range: Some((10, 20)),
				navigable: true,
				score: None,
				match_reason: None,
				source: None,
			},
			direction: code_moniker_query::UsageDirection::Incoming,
			include_descendants: true,
			targets: 2,
			rows: vec![
				UsageDto {
					root: "/workspace".to_string(),
					direction: code_moniker_query::UsageDirection::Incoming,
					reference: "reference:2:1".to_string(),
					kind: "calls".to_string(),
					actor: "missing context".to_string(),
					context: "symbol:2:3".to_string(),
					endpoint: target_uri.to_string(),
					file: "src/model.rs".to_string(),
					prefix: "src".to_string(),
					location: "src/model.rs:L7".to_string(),
					line_range: Some((7, 7)),
					via: None,
				},
				UsageDto {
					root: "/workspace".to_string(),
					direction: code_moniker_query::UsageDirection::Incoming,
					reference: "reference:2:2".to_string(),
					kind: "calls".to_string(),
					actor: "from_event(event)".to_string(),
					context: wrapper_uri.to_string(),
					endpoint: target_uri.to_string(),
					file: "src/model.rs".to_string(),
					prefix: "src".to_string(),
					location: "src/model.rs:L8".to_string(),
					line_range: Some((8, 8)),
					via: None,
				},
			],
			total: 2,
			incoming_summary: None,
			outgoing_summary: None,
		};

		let cursor = QueryCursor::new(2, None);
		let calls = usage_next_calls(scheme, &request, Some(&cursor), &result);

		assert!(
			calls
				.iter()
				.any(|call| call.tool == "code_moniker_read"
					&& call.uri.as_deref() == Some(wrapper_uri)),
			"the known wrapper context must be directly navigable"
		);
		assert!(
			!calls.iter().any(|call| call.tool == "code_moniker_read"
				&& call.uri.as_deref() == Some("symbol:2:3")),
			"internal symbol ordinals must never become navigation calls"
		);
		assert!(
			calls
				.iter()
				.any(|call| call.arguments.contains("include_descendants=true")),
			"pagination must preserve owner roll-up scope"
		);
		assert!(
			calls
				.iter()
				.all(|call| call.arguments.contains("compact=true")
					&& call.arguments.contains("budget=\"small\"")),
			"continuation calls must preserve the output profile"
		);
	}

	#[test]
	fn usage_volume_profiles_bound_pages_evidence_and_context_before_rendering() {
		assert_eq!(super::usage_evidence_limit(OutputBudget::Small), 4);
		assert_eq!(super::usage_evidence_limit(OutputBudget::Medium), 8);
		assert_eq!(super::usage_evidence_limit(OutputBudget::Full), 12);
		assert_eq!(super::usage_context_lines(OutputBudget::Small), 2);
		assert_eq!(super::usage_context_lines(OutputBudget::Medium), 4);
		assert_eq!(super::usage_context_lines(OutputBudget::Full), 8);
		let request = super::usage_request_from_arguments(
			&serde_json::json!({
				"uri": "rs:App",
				"limit": 500,
				"max_evidence": 12,
				"context_lines": 8
			}),
			AgentOutputOptions {
				compact: true,
				budget: OutputBudget::Small,
			},
		)
		.expect("small usages request");
		assert_eq!(request.paging.limit, 20);
		assert_eq!(request.max_evidence, 4);
		assert_eq!(request.context_lines, 2);
	}
}
