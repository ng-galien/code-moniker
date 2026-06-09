use std::path::{Path, PathBuf};

use code_moniker_core::lang::Lang;
use serde_json::{Value, json};

use super::scope::{
	Paging, append_call_bool_arg, append_call_number_arg, append_call_string_arg, string_list,
};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use code_moniker_check::{self as check, RuleSeverity};

use crate::mcp::context::McpContext;
use crate::{DEFAULT_SCHEME, Exit};

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
			"required": ["uri"],
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
}

fn rules_request_from_arguments(arguments: &Value) -> anyhow::Result<RulesRequest> {
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
		paging: Paging::from_arguments(arguments)?,
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
	let langs = workspace_languages(context, &request.langs)?;
	let rules_path = resolve_rules_path(context, &request.rules)?;
	let mut specs = check::RuleSetRequest::with_rules(rules_path, DEFAULT_SCHEME)
		.with_profile(request.profile.clone())
		.compiled_specs_for_langs(langs)?;
	specs.retain(|spec| {
		request.severities.is_empty() || request.severities.contains(&spec.severity)
	});
	specs.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
	let (start, end, next) = request.paging.window(&specs);
	let mut output = String::new();
	output.push_str(&format!("uri: {}\n", normalize_rules_uri(context.scheme())));
	if let Some(next) = next {
		output.push_str(&format!(
			"completeness: partial (rules {start}-{end} of {}, next cursor {next})\n",
			specs.len()
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("rules: {}\n", specs.len()));
	output.push_str(&format!("limit: {}\n\n", request.paging.limit));
	render_rules_scope(&mut output, request);
	output.push_str("rules:\n");
	if specs.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for spec in specs.iter().take(end).skip(start) {
			render_rule_spec(&mut output, spec);
		}
	}
	output.push_str("\nnext:\n");
	if let Some(next) = next {
		append_rules_next_call(
			&mut output,
			context.scheme(),
			request,
			RulesAction::List,
			request.paging.limit,
			Some(next),
		);
	}
	append_rules_next_call(
		&mut output,
		context.scheme(),
		request,
		RulesAction::Run,
		20,
		None,
	);
	Ok(output)
}

fn run_rules(context: &McpContext, request: &RulesRequest) -> anyhow::Result<String> {
	ensure_workspace_uri(&request.uri, context.scheme())?;
	let rules_path = resolve_rules_path(context, &request.rules)?;
	let outcomes = run_rules_for_roots(context, request, &rules_path);
	let exit = aggregate_exit(&outcomes);
	let mut output = String::new();
	output.push_str(&format!("uri: {}\n", normalize_rules_uri(context.scheme())));
	output.push_str("completeness: full\n");
	output.push_str("action: run\n");
	output.push_str(&format!("exit: {}\n", exit_label(exit)));
	output.push_str(&format!("limit: {}\n\n", request.paging.limit));
	render_rules_scope(&mut output, request);
	output.push_str("report:\n");
	for outcome in &outcomes {
		output.push_str(&format!("  root: {}\n", outcome.root.display()));
		render_prefixed_block(&mut output, outcome.stdout.trim_end(), "    ");
		if !outcome.stderr.trim().is_empty() {
			output.push_str("  stderr:\n");
			render_prefixed_block(&mut output, outcome.stderr.trim_end(), "    ");
		}
	}
	output.push_str("\nnext:\n");
	append_rules_next_call(
		&mut output,
		context.scheme(),
		request,
		RulesAction::List,
		50,
		None,
	);
	Ok(output)
}

struct RulesRunOutcome {
	root: PathBuf,
	exit: Exit,
	stdout: String,
	stderr: String,
}

fn run_rules_for_roots(
	context: &McpContext,
	request: &RulesRequest,
	rules_path: &Path,
) -> Vec<RulesRunOutcome> {
	context
		.opts()
		.paths
		.iter()
		.map(|root| run_rules_for_root(root, request, rules_path))
		.collect()
}

fn run_rules_for_root(root: &Path, request: &RulesRequest, rules_path: &Path) -> RulesRunOutcome {
	let rules = check::RuleSetRequest::with_rules(rules_path, DEFAULT_SCHEME)
		.with_profile(request.profile.clone());
	let check_request = check::CheckRequest::new(root.to_path_buf(), rules)
		.with_report(request.report)
		.with_files(request.files.clone());
	let mut stdout = Vec::new();
	let mut stderr = Vec::new();
	let exit = crate::check::run_text_request(
		check_request,
		request.report,
		Some(request.paging.limit),
		&mut stdout,
		&mut stderr,
	);
	RulesRunOutcome {
		root: root.to_path_buf(),
		exit,
		stdout: String::from_utf8_lossy(&stdout).to_string(),
		stderr: String::from_utf8_lossy(&stderr).to_string(),
	}
}

fn aggregate_exit(outcomes: &[RulesRunOutcome]) -> Exit {
	if outcomes
		.iter()
		.any(|outcome| outcome.exit == Exit::UsageError)
	{
		Exit::UsageError
	} else if outcomes.iter().any(|outcome| outcome.exit == Exit::NoMatch) {
		Exit::NoMatch
	} else {
		Exit::Match
	}
}

fn workspace_languages(context: &McpContext, filter: &[String]) -> anyhow::Result<Vec<Lang>> {
	let snapshot = context.index().catalog_snapshot()?;
	let mut langs = snapshot
		.catalog
		.sources
		.iter()
		.filter_map(|source| source.language.as_deref())
		.filter(|tag| filter.is_empty() || filter.iter().any(|allowed| allowed == tag))
		.filter_map(Lang::from_tag)
		.collect::<Vec<_>>();
	langs.sort_by_key(|lang| lang.tag());
	langs.dedup();
	Ok(langs)
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

fn resolve_rules_path(context: &McpContext, rules: &Path) -> anyhow::Result<PathBuf> {
	if rules.is_absolute() {
		return Ok(rules.to_path_buf());
	}
	Ok(workspace_config_root(context)?.join(rules))
}

fn workspace_config_root(context: &McpContext) -> anyhow::Result<PathBuf> {
	let roots = context
		.opts()
		.paths
		.iter()
		.map(|path| {
			if path.is_dir() {
				path.clone()
			} else {
				path.parent()
					.unwrap_or_else(|| Path::new("."))
					.to_path_buf()
			}
		})
		.collect::<Vec<_>>();
	let Some(first) = roots.first() else {
		anyhow::bail!("rules require at least one workspace root");
	};
	let mut common = first.clone();
	for root in roots.iter().skip(1) {
		while !root.starts_with(&common) {
			if !common.pop() {
				anyhow::bail!("cannot find common root for MCP rules");
			}
		}
	}
	Ok(common)
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
	cursor: Option<usize>,
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
		append_call_number_arg(output, "cursor", cursor);
	}
	output.push('\n');
}

fn render_rule_spec(output: &mut String, spec: &check::CompiledRuleSpec) {
	output.push_str(&format!(
		"  - {} [{}] domain={}\n",
		spec.rule_id,
		spec.severity.as_str(),
		spec.domain
	));
	if let Some(message) = &spec.message {
		output.push_str(&format!("    message: {message}\n"));
	}
	if let Some(rationale) = &spec.rationale {
		output.push_str("    rationale:\n");
		render_indented_block(output, rationale.trim());
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

fn exit_label(exit: Exit) -> &'static str {
	match exit {
		Exit::Match => "match",
		Exit::NoMatch => "violations",
		Exit::UsageError => "usage_error",
	}
}
