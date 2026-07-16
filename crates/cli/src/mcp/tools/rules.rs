use std::path::{Path, PathBuf};

use code_moniker_query::{
	Query, QueryRequest, QueryResult, RuleDto, RulesCheckQuery, RulesCheckResult,
	RulesCheckRootResult, RulesListQuery,
};
use serde_json::{Value, json};

use super::common::compact_argument;
use super::scope::{
	Paging, append_call_bool_arg, append_call_cursor_arg, append_call_number_arg,
	append_call_string_arg, string_list,
};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use code_moniker_check::RuleSeverity;

use crate::mcp::context::McpContext;

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
				"compact": {
					"type": "boolean",
					"default": true,
					"description": "Use minimal navigation output. Defaults true; false preserves guided next calls."
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

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let request = rules_request_from_arguments(arguments).map_err(ToolError::failed)?;
		let text = match request.action {
			RulesAction::List => list_rules(context, &request),
			RulesAction::Run => run_rules(context, &request),
		}
		.map_err(ToolError::failed)?;
		Ok(ToolResult {
			text,
			is_error: false,
		})
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
	compact: bool,
}

fn rules_request_from_arguments(arguments: &Value) -> anyhow::Result<RulesRequest> {
	let action = rules_action_from_arguments(arguments)?;
	let compact = compact_argument(arguments)?;
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
		paging: Paging::from_arguments_for_output(arguments, compact)?,
		compact,
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

fn list_rules(context: &McpContext, request: &RulesRequest) -> anyhow::Result<String> {
	ensure_workspace_uri(&request.uri, context.scheme())?;
	let response = context.query(QueryRequest {
		query: Query::RulesList(RulesListQuery {
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
		consistency: code_moniker_query::Consistency::Current,
		page: request.paging.daemon_page(),
	})?;
	let QueryResult::RulesList(result) = response.result else {
		anyhow::bail!("unexpected daemon response for rules list");
	};
	let start = request.paging.cursor.min(result.total);
	let end = start.saturating_add(result.rows.len()).min(result.total);
	let mut output = String::new();
	output.push_str(&format!("uri: {}\n", normalize_rules_uri(context.scheme())));
	if let Some(next) = response.next_cursor.as_ref() {
		output.push_str(&format!(
			"completeness: partial (rules {start}-{end} of {}, next cursor {})\n",
			result.total, next.offset
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("rules: {}\n", result.total));
	output.push_str(&format!("limit: {}\n\n", request.paging.limit));
	render_rules_scope(&mut output, request);
	output.push_str("rules:\n");
	if result.rows.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for spec in &result.rows {
			render_rule_dto(&mut output, spec);
		}
	}
	if response.next_cursor.is_some() || !request.compact {
		output.push_str("\nnext:\n");
	}
	if let Some(next) = response.next_cursor.as_ref() {
		append_rules_next_call(
			&mut output,
			context.scheme(),
			request,
			RulesAction::List,
			request.paging.limit,
			Some(next),
		);
	}
	if !request.compact {
		append_rules_next_call(
			&mut output,
			context.scheme(),
			request,
			RulesAction::Run,
			20,
			None,
		);
	}
	Ok(output)
}

fn run_rules(context: &McpContext, request: &RulesRequest) -> anyhow::Result<String> {
	ensure_workspace_uri(&request.uri, context.scheme())?;
	let response = context.query(QueryRequest {
		query: Query::RulesCheck(RulesCheckQuery {
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
		consistency: code_moniker_query::Consistency::RefreshIfStale,
		page: request.paging.daemon_page(),
	})?;
	let QueryResult::RulesCheck(result) = response.result else {
		anyhow::bail!("unexpected daemon response for rules run");
	};
	let mut output = String::new();
	output.push_str(&format!("uri: {}\n", normalize_rules_uri(context.scheme())));
	if let Some(next) = response.next_cursor.as_ref() {
		output.push_str(&format!(
			"completeness: partial (rules rows next cursor {})\n",
			next.offset
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str("action: run\n");
	output.push_str(&format!("exit: {}\n", result.exit));
	output.push_str(&format!("limit: {}\n\n", request.paging.limit));
	render_rules_scope(&mut output, request);
	output.push_str("report:\n");
	render_rules_check_result(&mut output, &result);
	if let Some(next) = response.next_cursor.as_ref() {
		output.push_str("\nnext:\n");
		append_rules_next_call(
			&mut output,
			context.scheme(),
			request,
			RulesAction::Run,
			request.paging.limit,
			Some(next),
		);
	} else if !request.compact {
		output.push_str("\nnext:\n");
	}
	if !request.compact {
		append_rules_next_call(
			&mut output,
			context.scheme(),
			request,
			RulesAction::List,
			50,
			None,
		);
	}
	Ok(output)
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

fn render_rules_scope(output: &mut String, request: &RulesRequest) {
	output.push_str("scope:\n");
	output.push_str(&format!(
		"  profile: {}\n",
		request.profile.as_deref().unwrap_or("<all>")
	));
	output.push_str(&format!("  rules: {}\n", request.rules.display()));
	if !request.langs.is_empty() {
		output.push_str(&format!("  lang: {}\n", request.langs.join(", ")));
	}
	if !request.severities.is_empty() {
		let severities = request
			.severities
			.iter()
			.map(|severity| severity.as_str())
			.collect::<Vec<_>>();
		output.push_str(&format!("  severity: {}\n", severities.join(", ")));
	}
	if !request.files.is_empty() {
		let files = request
			.files
			.iter()
			.map(|file| file.display().to_string())
			.collect::<Vec<_>>();
		output.push_str(&format!("  file: {}\n", files.join(", ")));
	}
	output.push('\n');
}

fn append_rules_next_call(
	output: &mut String,
	scheme: &str,
	request: &RulesRequest,
	action: RulesAction,
	limit: usize,
	cursor: Option<&code_moniker_query::QueryCursor>,
) {
	output.push_str(&format!("  - code_moniker_rules uri=\"{scheme}workspace\""));
	append_call_string_arg(
		output,
		"action",
		match action {
			RulesAction::List => "list",
			RulesAction::Run => "run",
		},
	);
	if let Some(profile) = &request.profile {
		append_call_string_arg(output, "profile", profile);
	}
	if request.rules != Path::new(".code-moniker.toml") {
		append_call_string_arg(output, "rules", &request.rules.display().to_string());
	}
	match action {
		RulesAction::List => {
			for lang in &request.langs {
				append_call_string_arg(output, "lang", lang);
			}
			for severity in &request.severities {
				append_call_string_arg(output, "severity", severity.as_str());
			}
		}
		RulesAction::Run => {
			for file in &request.files {
				append_call_string_arg(output, "file", &file.display().to_string());
			}
			if !request.report {
				append_call_bool_arg(output, "report", false);
			}
		}
	}
	append_call_number_arg(output, "limit", limit);
	if let Some(cursor) = cursor {
		append_call_cursor_arg(output, "cursor", cursor);
	}
	if !request.compact {
		append_call_bool_arg(output, "compact", false);
	}
	output.push('\n');
}

fn render_rule_dto(output: &mut String, spec: &RuleDto) {
	output.push_str(&format!(
		"  - {} [{}] domain={}\n",
		spec.id, spec.severity, spec.domain
	));
	if let Some(message) = &spec.message {
		output.push_str(&format!("    message: {message}\n"));
	}
	if let Some(rationale) = &spec.rationale {
		output.push_str("    rationale:\n");
		render_indented_block(output, rationale.trim());
	}
}

fn render_rules_check_result(output: &mut String, result: &RulesCheckResult) {
	for root in &result.roots {
		render_rules_root_summary(output, root);
	}
	if !result.violations.is_empty() {
		output.push_str("    violations:\n");
		for violation in &result.violations {
			output.push_str(&format!(
				"    - {}:{}-{} [{}] {}: {}\n",
				violation.path,
				violation.lines.0,
				violation.lines.1,
				violation.rule_id,
				violation.severity,
				violation.message
			));
		}
	}
	if !result.errors.is_empty() {
		output.push_str("    errors:\n");
		for error in &result.errors {
			output.push_str(&format!("    - {}: {}\n", error.path, error.error));
		}
	}
	if !result.rule_reports.is_empty() {
		output.push_str(&format!(
			"    rule_reports: {}\n",
			result.rule_reports.len()
		));
	}
}

fn render_rules_root_summary(output: &mut String, root: &RulesCheckRootResult) {
	output.push_str(&format!("  root: {}\n", root.root));
	output.push_str(&format!(
		"    {} violation(s), {} warning(s), {} error(s)\n",
		root.summary.total_violations, root.summary.total_warnings, root.summary.total_errors
	));
	for failed in &root.summary.failed_rules {
		output.push_str(&format!(
			"    - {}: {} {} violation(s)\n",
			failed.rule_id, failed.severity, failed.violations
		));
	}
}

fn render_indented_block(output: &mut String, text: &str) {
	render_prefixed_block(output, text, "  ");
}

fn render_prefixed_block(output: &mut String, text: &str, prefix: &str) {
	if text.is_empty() {
		output.push_str(prefix);
		output.push_str("<empty>\n");
		return;
	}
	for line in text.lines() {
		output.push_str(prefix);
		output.push_str(line);
		output.push('\n');
	}
}
