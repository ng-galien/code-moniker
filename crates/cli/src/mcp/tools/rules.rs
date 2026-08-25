use std::path::{Path, PathBuf};

use code_moniker_query::{
	Query, QueryResult, RuleDto, RulesCheckQuery, RulesCheckResult, RulesListQuery,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::common::{AgentOutputOptions, OutputBudget};
use super::scope::{
	Paging, append_call_bool_arg, append_call_cursor_arg, append_call_number_arg,
	append_call_string_arg, string_list,
};
use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};
use code_moniker_check::RuleSeverity;

use crate::mcp::context::McpContext;
use crate::presentation::TemplateOutput;
use crate::presentation::rules as rules_presentation;

const DEFAULT_RULES_URI: &str = "workspace";

pub(super) struct RulesTool;

impl RulesTool {
	pub(super) const NAME: &'static str = "code_moniker_rules";

	const DESCRIPTION: &'static str = concat!(
		"When to use: understand or run the project's code-moniker rules. ",
		"Use this to inspect active guardrails, read scoped rationales, or execute the same check an agent hook would run.\n",
		"\n",
		"Rules from code-moniker.\n",
		"  action=list — list compiled rules for languages present in the workspace, with messages and rationales\n",
		"  action=run  — run code-moniker check on the UI workspace, optionally file-scoped\n",
		"Keep this as the rules domain: list, rationale, and execution are facets of the same project contract."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"action": {
					"type": "string",
					"enum": ["list", "run"],
					"description": "list active rules or run check."
				},
				"uri": {
					"type": "string",
					"description": "workspace | code+moniker://workspace"
				},
				"profile": {
					"type": "string",
					"description": "Named rule profile, for example agent or smells."
				},
				"rules": {
					"type": "string",
					"description": "Rules TOML path. Defaults to .code-moniker.toml."
				},
				"lang": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Language tag(s), OR-combined, for action=list."
				},
				"severity": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "error|warn filter for action=list."
				},
				"file": {
					"oneOf": [
						{ "type": "string" },
						{ "type": "array", "items": { "type": "string" } }
					],
					"description": "Touched file path(s), relative to the workspace root, for action=run."
				},
				"report": {
					"type": "boolean",
					"description": "Include per-rule observability when action=run. Defaults true."
				},
				"limit": {
					"type": "integer",
					"minimum": 1,
					"maximum": super::scope::MAX_LIMIT,
					"description": "List page size, or max violations for action=run."
				},
				"cursor": {
					"oneOf": [{ "type": "integer" }, { "type": "string" }],
					"description": "Opaque row offset returned in next calls for action=list."
				}
			},
			"additionalProperties": false
		})
	}
}

impl McpTool for RulesTool {
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
		let request = rules_request_from_arguments(arguments, output.agent_options())
			.map_err(ToolError::failed)?;
		match request.action {
			RulesAction::List => list_rules(context, &request).map(ToolResult::templated),
			RulesAction::Run => run_rules(context, &request).map(ToolResult::templated),
		}
		.map_err(ToolError::failed)
	}
}

struct RulesRequest {
	action: RulesAction,
	uri: String,
	profile: Option<String>,
	rules: PathBuf,
	langs: Vec<String>,
	severities: Vec<RuleSeverity>,
	files: Vec<PathBuf>,
	report: bool,
	paging: Paging,
	output: AgentOutputOptions,
}

fn rules_request_from_arguments(
	arguments: &Value,
	output: AgentOutputOptions,
) -> anyhow::Result<RulesRequest> {
	let action = rules_action_from_arguments(arguments)?;
	let langs = string_list(arguments, "lang")?
		.into_iter()
		.map(|lang| lang.to_ascii_lowercase())
		.collect::<Vec<_>>();
	let severities = string_list(arguments, "severity")?
		.into_iter()
		.map(|severity| parse_severity(&severity))
		.collect::<anyhow::Result<Vec<_>>>()?;
	if action == RulesAction::Run && (!langs.is_empty() || !severities.is_empty()) {
		anyhow::bail!("lang and severity filters apply to action=list, not action=run");
	}
	Ok(RulesRequest {
		action,
		uri: arguments
			.get("uri")
			.and_then(Value::as_str)
			.unwrap_or(DEFAULT_RULES_URI)
			.to_string(),
		profile: arguments
			.get("profile")
			.and_then(Value::as_str)
			.map(ToOwned::to_owned),
		rules: arguments
			.get("rules")
			.and_then(Value::as_str)
			.map(PathBuf::from)
			.unwrap_or_else(|| PathBuf::from(".code-moniker.toml")),
		langs,
		severities,
		files: string_list(arguments, "file")?
			.into_iter()
			.map(PathBuf::from)
			.collect(),
		report: arguments
			.get("report")
			.and_then(Value::as_bool)
			.unwrap_or(true),
		paging: Paging::from_arguments_for_volume(arguments, output)?,
		output,
	})
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RulesAction {
	List,
	Run,
}

fn rules_action_from_arguments(arguments: &Value) -> anyhow::Result<RulesAction> {
	match arguments
		.get("action")
		.and_then(Value::as_str)
		.unwrap_or("list")
	{
		"list" => Ok(RulesAction::List),
		"run" => Ok(RulesAction::Run),
		action => anyhow::bail!("unknown rules action `{action}`"),
	}
}

fn list_rules(context: &McpContext, request: &RulesRequest) -> anyhow::Result<TemplateOutput> {
	ensure_workspace_uri(&request.uri, context.scheme())?;
	let response = context.query_refreshed(
		Query::RulesList(RulesListQuery {
			workspace: None,
			profile: request.profile.clone(),
			rules: Some(request.rules.display().to_string()),
			lang: request.langs.clone(),
			severity: request
				.severities
				.iter()
				.map(|severity| severity.as_str().to_string())
				.collect(),
		}),
		request.paging.daemon_page(),
	)?;
	let QueryResult::RulesList(result) = response.result else {
		anyhow::bail!("unexpected daemon response for rules list");
	};
	let start = request.paging.cursor.min(result.total);
	let end = start.saturating_add(result.rows.len()).min(result.total);
	let mut next_calls = Vec::new();
	if let Some(next) = response.next_cursor.as_ref() {
		next_calls.push(rules_next_call(
			context.scheme(),
			request,
			RulesAction::List,
			request.paging.limit,
			Some(next),
		));
	}
	if !request.output.compact {
		next_calls.push(rules_next_call(
			context.scheme(),
			request,
			RulesAction::Run,
			20,
			None,
		));
	}
	let view = McpRulesListView {
		uri: normalize_rules_uri(context.scheme()),
		partial: response.next_cursor.is_some(),
		start,
		end,
		total: result.total,
		next_cursor: response.next_cursor.as_ref().map(|cursor| cursor.offset),
		limit: request.paging.limit,
		volume: request.output.budget.as_str(),
		scope: rules_scope_view(request),
		rules: &result.rows,
		next_calls,
	};
	rules_presentation::mcp_list(&view)
}

fn run_rules(context: &McpContext, request: &RulesRequest) -> anyhow::Result<TemplateOutput> {
	ensure_workspace_uri(&request.uri, context.scheme())?;
	let response = context.query_refreshed(
		Query::RulesCheck(RulesCheckQuery {
			workspace: None,
			profile: request.profile.clone(),
			rules: Some(request.rules.display().to_string()),
			file: request
				.files
				.iter()
				.map(|file| file.display().to_string())
				.collect(),
			report: request.report,
		}),
		request.paging.daemon_page(),
	)?;
	let QueryResult::RulesCheck(result) = response.result else {
		anyhow::bail!("unexpected daemon response for rules run");
	};
	let mut next_calls = Vec::new();
	if let Some(next) = response.next_cursor.as_ref() {
		next_calls.push(rules_next_call(
			context.scheme(),
			request,
			RulesAction::Run,
			request.paging.limit,
			Some(next),
		));
	}
	if !request.output.compact {
		next_calls.push(rules_next_call(
			context.scheme(),
			request,
			RulesAction::List,
			50,
			None,
		));
	}
	let view = McpRulesRunView {
		uri: normalize_rules_uri(context.scheme()),
		partial: response.next_cursor.is_some(),
		next_cursor: response.next_cursor.as_ref().map(|cursor| cursor.offset),
		generation: response.generation.map(|generation| generation.0),
		limit: request.paging.limit,
		volume: request.output.budget.as_str(),
		scope: rules_scope_view(request),
		result: &result,
		next_calls,
	};
	let candidates = result
		.rule_reports
		.iter()
		.filter_map(|report| report.path_analysis.as_ref())
		.flat_map(|path| &path.witness)
		.flat_map(|step| [step.source.as_str(), step.target.as_str()]);
	Ok(rules_presentation::mcp_run(&view)?.with_monikers(candidates))
}

fn ensure_workspace_uri(uri: &str, scheme: &str) -> anyhow::Result<()> {
	let value = uri.trim();
	if value.is_empty()
		|| value == DEFAULT_RULES_URI
		|| value == format!("{scheme}workspace")
		|| value == format!("{scheme}.")
		|| value == scheme.trim_end_matches('/')
	{
		return Ok(());
	}
	anyhow::bail!("unsupported URI; use workspace or {scheme}workspace")
}

fn normalize_rules_uri(scheme: &str) -> String {
	format!("{scheme}workspace/rules")
}

fn parse_severity(value: &str) -> anyhow::Result<RuleSeverity> {
	match value {
		"error" => Ok(RuleSeverity::Error),
		"warn" | "warning" => Ok(RuleSeverity::Warn),
		_ => anyhow::bail!("unknown severity `{value}`; expected error or warn"),
	}
}

#[derive(Serialize)]
struct McpRulesScopeView<'a> {
	profile: &'a str,
	rules: String,
	langs: &'a [String],
	severities: Vec<&'static str>,
	files: Vec<String>,
}

#[derive(Serialize)]
struct McpRulesListView<'a> {
	uri: String,
	partial: bool,
	start: usize,
	end: usize,
	total: usize,
	next_cursor: Option<usize>,
	limit: usize,
	volume: &'static str,
	scope: McpRulesScopeView<'a>,
	rules: &'a [RuleDto],
	next_calls: Vec<McpRulesNextCall>,
}

#[derive(Serialize)]
struct McpRulesRunView<'a> {
	uri: String,
	partial: bool,
	next_cursor: Option<usize>,
	generation: Option<u64>,
	limit: usize,
	volume: &'static str,
	scope: McpRulesScopeView<'a>,
	result: &'a RulesCheckResult,
	next_calls: Vec<McpRulesNextCall>,
}

#[derive(Serialize)]
struct McpRulesNextCall {
	uri: String,
	arguments: String,
}

fn rules_scope_view(request: &RulesRequest) -> McpRulesScopeView<'_> {
	McpRulesScopeView {
		profile: request.profile.as_deref().unwrap_or("all"),
		rules: request.rules.display().to_string(),
		langs: &request.langs,
		severities: request
			.severities
			.iter()
			.map(|severity| severity.as_str())
			.collect(),
		files: request
			.files
			.iter()
			.map(|file| file.display().to_string())
			.collect(),
	}
}

fn rules_next_call(
	scheme: &str,
	request: &RulesRequest,
	action: RulesAction,
	limit: usize,
	cursor: Option<&code_moniker_query::QueryCursor>,
) -> McpRulesNextCall {
	let mut arguments = String::new();
	append_call_string_arg(
		&mut arguments,
		"action",
		match action {
			RulesAction::List => "list",
			RulesAction::Run => "run",
		},
	);
	if let Some(profile) = &request.profile {
		append_call_string_arg(&mut arguments, "profile", profile);
	}
	if request.rules != Path::new(".code-moniker.toml") {
		append_call_string_arg(
			&mut arguments,
			"rules",
			&request.rules.display().to_string(),
		);
	}
	match action {
		RulesAction::List => {
			for lang in &request.langs {
				append_call_string_arg(&mut arguments, "lang", lang);
			}
			for severity in &request.severities {
				append_call_string_arg(&mut arguments, "severity", severity.as_str());
			}
		}
		RulesAction::Run => {
			for file in &request.files {
				append_call_string_arg(&mut arguments, "file", &file.display().to_string());
			}
			if !request.report {
				append_call_bool_arg(&mut arguments, "report", false);
			}
		}
	}
	append_call_number_arg(&mut arguments, "limit", limit);
	if let Some(cursor) = cursor {
		append_call_cursor_arg(&mut arguments, "cursor", cursor);
	}
	if request.output.budget != OutputBudget::Small {
		append_call_string_arg(&mut arguments, "budget", request.output.budget.as_str());
	}
	if !request.output.compact {
		append_call_bool_arg(&mut arguments, "compact", false);
	}
	McpRulesNextCall {
		uri: format!("{scheme}workspace"),
		arguments,
	}
}
