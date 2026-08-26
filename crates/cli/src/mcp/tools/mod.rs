pub(in crate::mcp) mod common;
pub(super) mod context;
pub(super) mod diff;
pub(super) mod graph;
pub(super) mod notes;
pub(super) mod query;
pub(super) mod read;
pub(super) mod refresh;
pub(super) mod rules;
pub(in crate::mcp) mod scope;
pub(super) mod search;
pub(in crate::mcp) mod symbols;
pub(in crate::mcp) mod usages;

use serde::Serialize;
use serde_json::Value;

use super::context::McpContext;
use crate::presentation::problem as problem_presentation;
use crate::presentation::{RenderOptions, TemplateOutput};
use context::ContextTool;
use diff::DiffTool;
use graph::GraphTool;
use notes::NotesTool;
use query::QueryTool;
use read::ReadTool;
use refresh::RefreshTool;
use rmcp::model::{JsonObject, Tool};
use rules::RulesTool;
use search::SearchTool;
use symbols::SymbolsTool;
use usages::UsagesTool;

pub(super) struct ToolDescriptor {
	pub(super) name: &'static str,
	pub(super) description: &'static str,
	pub(super) input_schema: Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OutputContract {
	Agent,
	Plain,
}

impl OutputContract {
	fn decorates_schema(self, schema: &mut Value) {
		common::add_output_format_schema(schema);
		if self == Self::Agent {
			common::add_compact_output_schema(schema);
			common::add_output_budget_schema(schema);
		}
	}

	fn output_options(self, arguments: &Value) -> Result<OutputOptions, ToolError> {
		let format = common::OutputFormat::from_arguments(arguments).map_err(ToolError::failed)?;
		match self {
			Self::Agent => common::AgentOutputOptions::for_format(arguments, format)
				.map(|agent| OutputOptions::agent(agent, format))
				.map_err(ToolError::failed),
			Self::Plain => Ok(OutputOptions::plain(format)),
		}
	}

	fn finalize(
		self,
		result: ToolResult,
		options: OutputOptions,
		scheme: &str,
		runtime: Option<&str>,
	) -> Result<ToolResult, ToolError> {
		match (options.format, self) {
			(common::OutputFormat::Json, _) => result.into_json(),
			(common::OutputFormat::Text, Self::Plain) => Ok(result.into_plain_text()),
			(common::OutputFormat::Text, Self::Agent) => {
				result.into_agent_text(options.agent_options(), scheme, runtime)
			}
		}
	}
}

#[derive(Clone, Copy, Debug)]
pub(super) struct OutputOptions {
	agent: Option<common::AgentOutputOptions>,
	format: common::OutputFormat,
}

impl OutputOptions {
	fn agent(options: common::AgentOutputOptions, format: common::OutputFormat) -> Self {
		Self {
			agent: Some(options),
			format,
		}
	}

	fn plain(format: common::OutputFormat) -> Self {
		Self {
			agent: None,
			format,
		}
	}

	pub(super) fn agent_options(self) -> common::AgentOutputOptions {
		self.agent
			.expect("agent output options require OutputContract::Agent")
	}
}

impl ToolDescriptor {
	#[cfg(test)]
	fn into_mcp_value(mut self, contract: OutputContract) -> Value {
		contract.decorates_schema(&mut self.input_schema);
		serde_json::json!({
			"name": self.name,
			"description": self.description,
			"inputSchema": self.input_schema,
		})
	}
}

#[derive(Debug)]
pub(super) struct ToolResult {
	pub(super) text: String,
	pub(super) is_error: bool,
	pub(super) structured_content: Option<Value>,
	template: Option<TemplateOutput>,
}

impl ToolResult {
	pub(super) fn success(text: impl Into<String>) -> Self {
		Self {
			text: text.into(),
			is_error: false,
			structured_content: None,
			template: None,
		}
	}

	pub(super) fn templated(template: TemplateOutput) -> Self {
		Self {
			text: String::new(),
			is_error: false,
			structured_content: None,
			template: Some(template),
		}
	}

	pub(super) fn error(text: impl Into<String>) -> Self {
		Self {
			text: text.into(),
			is_error: true,
			structured_content: None,
			template: None,
		}
	}

	pub(super) fn with_structured_content(mut self, value: Value) -> Self {
		self.structured_content = Some(value);
		self
	}

	fn into_json(mut self) -> Result<Self, ToolError> {
		let structured_content = self
			.structured_content
			.take()
			.or_else(|| {
				self.template
					.as_ref()
					.map(|template| template.context().clone())
			})
			.ok_or_else(|| ToolError::failed("JSON output requires a structured result"))?;
		self.text.clear();
		self.template = None;
		self.structured_content = Some(structured_content);
		Ok(self)
	}

	fn into_plain_text(mut self) -> Self {
		self.structured_content = None;
		self.template = None;
		self
	}

	fn into_agent_text(
		mut self,
		agent: common::AgentOutputOptions,
		scheme: &str,
		runtime: Option<&str>,
	) -> Result<Self, ToolError> {
		self.structured_content = None;
		let template = self.template.take().ok_or_else(|| {
			ToolError::failed("agent output must be produced by the shared template renderer")
		})?;
		self.text = TemplateOutput::render(
			&template,
			RenderOptions {
				compact: agent.compact,
				scheme,
				runtime,
			},
		)
		.map_err(ToolError::failed)?;
		Ok(self)
	}

	pub(super) fn into_response_parts(self) -> (bool, String, Option<Value>) {
		(self.is_error, self.text, self.structured_content)
	}

	fn templated_error(template: TemplateOutput) -> Self {
		Self {
			text: String::new(),
			is_error: true,
			structured_content: None,
			template: Some(template),
		}
	}
}

pub(super) trait McpTool {
	fn descriptor(&self) -> ToolDescriptor;
	fn output_contract(&self) -> OutputContract;
	fn call(
		&self,
		context: &McpContext,
		arguments: &Value,
		output: OutputOptions,
	) -> Result<ToolResult, ToolError>;
}

pub(super) struct ToolError {
	kind: ToolErrorKind,
	message: String,
}

impl ToolError {
	pub(super) fn unknown_tool(name: &str) -> Self {
		Self {
			kind: ToolErrorKind::UnknownTool,
			message: format!("unknown tool: {name}"),
		}
	}

	pub(super) fn failed(error: impl std::fmt::Display) -> Self {
		Self {
			kind: ToolErrorKind::Failed,
			message: error.to_string(),
		}
	}

	pub(super) fn is_unknown_tool(&self) -> bool {
		matches!(self.kind, ToolErrorKind::UnknownTool)
	}
}

impl std::fmt::Display for ToolError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.message)
	}
}

impl std::fmt::Debug for ToolError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ToolError")
			.field("message", &self.message)
			.finish()
	}
}

impl std::error::Error for ToolError {}

enum ToolErrorKind {
	UnknownTool,
	Failed,
}

pub(super) struct ToolRegistry {
	read: ReadTool,
	context: ContextTool,
	diff: DiffTool,
	graph: GraphTool,
	notes: NotesTool,
	query: QueryTool,
	refresh: RefreshTool,
	rules: RulesTool,
	search: SearchTool,
	symbols: SymbolsTool,
	usages: UsagesTool,
}

impl ToolRegistry {
	pub(super) fn new() -> Self {
		Self {
			read: ReadTool,
			context: ContextTool,
			diff: DiffTool,
			graph: GraphTool,
			notes: NotesTool,
			query: QueryTool,
			refresh: RefreshTool,
			rules: RulesTool,
			search: SearchTool,
			symbols: SymbolsTool,
			usages: UsagesTool,
		}
	}

	#[cfg(test)]
	pub(super) fn descriptors(&self) -> Vec<Value> {
		self.all()
			.into_iter()
			.map(|tool| tool.descriptor().into_mcp_value(tool.output_contract()))
			.collect()
	}

	fn all(&self) -> [&dyn McpTool; 11] {
		[
			&self.read,
			&self.context,
			&self.query,
			&self.notes,
			&self.search,
			&self.symbols,
			&self.usages,
			&self.rules,
			&self.diff,
			&self.graph,
			&self.refresh,
		]
	}

	pub(super) fn tools(&self) -> Vec<Tool> {
		self.all()
			.into_iter()
			.map(|tool| tool.descriptor().into_rmcp_tool(tool.output_contract()))
			.collect()
	}

	pub(super) fn render_problem(
		&self,
		name: &str,
		arguments: &Value,
		scheme: &str,
		runtime: Option<&str>,
		message: &str,
	) -> Option<ToolResult> {
		let contract = self
			.all()
			.into_iter()
			.find(|tool| tool.descriptor().name == name)?
			.output_contract();
		finalize_problem(contract, name, arguments, scheme, runtime, message)
	}

	pub(super) fn call(
		&self,
		context: &McpContext,
		name: &str,
		arguments: &Value,
	) -> Result<ToolResult, ToolError> {
		let Some(tool) = self
			.all()
			.into_iter()
			.find(|tool| tool.descriptor().name == name)
		else {
			return Err(ToolError::unknown_tool(name));
		};
		execute_tool(tool, context, arguments)
	}
}

fn finalize_problem(
	contract: OutputContract,
	name: &str,
	arguments: &Value,
	scheme: &str,
	runtime: Option<&str>,
	message: &str,
) -> Option<ToolResult> {
	let options = contract
		.output_options(arguments)
		.or_else(|_| contract.output_options(&serde_json::json!({})))
		.ok()?;
	let view = mcp_problem_view(name, arguments, message);
	let result = match contract {
		OutputContract::Agent => problem_presentation::mcp(&view)
			.map(ToolResult::templated_error)
			.ok()?,
		OutputContract::Plain => {
			ToolResult::error(message).with_structured_content(serde_json::to_value(&view).ok()?)
		}
	};
	contract.finalize(result, options, scheme, runtime).ok()
}

fn execute_tool(
	tool: &dyn McpTool,
	context: &McpContext,
	arguments: &Value,
) -> Result<ToolResult, ToolError> {
	let contract = tool.output_contract();
	let options = contract.output_options(arguments)?;
	let result = tool.call(context, arguments, options)?;
	contract.finalize(
		result,
		options,
		context.scheme(),
		Some(context.runtime_label()),
	)
}

#[derive(Serialize)]
struct McpProblemView<'a> {
	uri: &'a str,
	tool: &'a str,
	problem: &'a str,
	fix_hint: &'static str,
}

#[cfg(test)]
fn agent_problem_result(
	tool: &str,
	arguments: &Value,
	message: &str,
) -> Result<ToolResult, ToolError> {
	problem_presentation::mcp(&mcp_problem_view(tool, arguments, message))
		.map(ToolResult::templated_error)
		.map_err(ToolError::failed)
}

fn mcp_problem_view<'a>(
	tool: &'a str,
	arguments: &'a Value,
	message: &'a str,
) -> McpProblemView<'a> {
	let uri = arguments
		.get("uri")
		.and_then(Value::as_str)
		.unwrap_or("workspace");
	let problem = if message
		.strip_prefix("symbol_not_found: symbol not found:")
		.is_some_and(|missing| missing.trim() == uri)
	{
		"symbol_not_found"
	} else {
		message
	};
	let fix_hint = if message.starts_with("workspace_mismatch:") {
		"Stop and connect to the project-owned Code Moniker MCP server for the expected roots."
	} else if message.starts_with("workspace_identity_required:") {
		"Retry the workspace read with `expected_roots` set to the current absolute workspace roots."
	} else if message.starts_with("symbol_not_found:") {
		"Discover the current moniker with `code_moniker_symbols`, then retry with that value."
	} else {
		"Retry with a supported URI and bounded arguments."
	};
	McpProblemView {
		uri,
		tool,
		problem,
		fix_hint,
	}
}

impl ToolDescriptor {
	fn into_rmcp_tool(mut self, contract: OutputContract) -> Tool {
		contract.decorates_schema(&mut self.input_schema);
		Tool::new(
			self.name,
			self.description,
			json_object_schema(self.input_schema),
		)
	}
}

fn json_object_schema(schema: Value) -> JsonObject {
	match schema {
		Value::Object(object) => object,
		_ => JsonObject::new(),
	}
}

#[cfg(test)]
mod tests {
	use serde::Serialize;

	use super::{OutputContract, ToolRegistry, ToolResult, agent_problem_result};
	use crate::presentation::TemplateOutput;

	#[derive(Serialize)]
	struct ContractFixture<'a> {
		uri: &'a str,
		count: usize,
	}

	const CONTRACT_TEMPLATE: &str = "# Fixture\n\n- uri: `{{ uri }}`\n- count: {{ count }}\n";

	fn fixture_result() -> ToolResult {
		ToolResult::templated(
			TemplateOutput::new(
				"mcp-contract-fixture.md.j2",
				CONTRACT_TEMPLATE,
				&ContractFixture {
					uri: "workspace",
					count: 2,
				},
			)
			.expect("fixture template"),
		)
	}

	#[test]
	fn agent_contract_selects_exactly_one_representation() {
		let text = OutputContract::Agent
			.finalize(
				fixture_result(),
				OutputContract::Agent
					.output_options(&serde_json::json!({}))
					.expect("text options"),
				"code+moniker://",
				None,
			)
			.expect("text representation");
		assert!(text.text.contains("# Fixture"));
		assert!(text.structured_content.is_none());

		let json = OutputContract::Agent
			.finalize(
				fixture_result(),
				OutputContract::Agent
					.output_options(&serde_json::json!({"format": "json"}))
					.expect("JSON options"),
				"code+moniker://",
				None,
			)
			.expect("JSON representation");
		assert!(json.text.is_empty());
		assert_eq!(json.structured_content.unwrap()["count"], 2);
	}

	#[test]
	fn structured_override_is_used_only_for_json() {
		let result = || {
			fixture_result().with_structured_content(serde_json::json!({
				"raw": "typed-query-result"
			}))
		};
		let text = OutputContract::Agent
			.finalize(
				result(),
				OutputContract::Agent
					.output_options(&serde_json::json!({"format": "text"}))
					.expect("text options"),
				"code+moniker://",
				None,
			)
			.expect("text representation");
		assert!(text.text.contains("# Fixture"));
		assert!(text.structured_content.is_none());

		let json = OutputContract::Agent
			.finalize(
				result(),
				OutputContract::Agent
					.output_options(&serde_json::json!({"format": "json"}))
					.expect("JSON options"),
				"code+moniker://",
				None,
			)
			.expect("JSON representation");
		assert!(json.text.is_empty());
		assert_eq!(
			json.structured_content.unwrap()["raw"],
			"typed-query-result"
		);
	}

	#[test]
	fn plain_contract_selects_exactly_one_representation() {
		let result = || {
			ToolResult::success("refreshed: generation 7")
				.with_structured_content(serde_json::json!({"generation": 7}))
		};
		let text = OutputContract::Plain
			.finalize(
				result(),
				OutputContract::Plain
					.output_options(&serde_json::json!({}))
					.expect("text options"),
				"code+moniker://",
				None,
			)
			.expect("text representation");
		assert_eq!(text.text, "refreshed: generation 7");
		assert!(text.structured_content.is_none());

		let json = OutputContract::Plain
			.finalize(
				result(),
				OutputContract::Plain
					.output_options(&serde_json::json!({"format": "json"}))
					.expect("JSON options"),
				"code+moniker://",
				None,
			)
			.expect("JSON representation");
		assert!(json.text.is_empty());
		assert_eq!(json.structured_content.unwrap()["generation"], 7);
	}

	#[test]
	fn agent_contract_rejects_pre_rendered_text() {
		let error = OutputContract::finalize(
			OutputContract::Agent,
			ToolResult::success("pre-rendered output"),
			OutputContract::Agent
				.output_options(&serde_json::json!({}))
				.expect("output options"),
			"code+moniker://",
			None,
		)
		.expect_err("pre-rendered agent text must not bypass the template renderer");

		assert!(error.to_string().contains("shared template renderer"));
	}

	#[test]
	fn agent_problem_uses_the_shared_markdown_template() {
		let uri = "code+moniker://./lang:rs/module:mcp/struct:Missing";
		let result = agent_problem_result(
			"code_moniker_read",
			&serde_json::json!({"uri": uri}),
			&format!("symbol_not_found: symbol not found: {uri}"),
		)
		.expect("problem DTO");
		let result = OutputContract::Agent
			.finalize(
				result,
				OutputContract::Agent
					.output_options(&serde_json::json!({}))
					.expect("output options"),
				"code+moniker://",
				Some("test-runtime"),
			)
			.expect("problem template");

		assert!(result.is_error);
		assert!(result.text.contains("symbol_not_found"), "{}", result.text);
		assert_eq!(result.text.matches(uri).count(), 0, "{}", result.text);
		crate::presentation::tests::validate_agent_markdown(&result.text, "Tool problem", false)
			.expect("problem Markdown");
	}

	#[test]
	fn known_tool_errors_honor_the_selected_representation() {
		let registry = ToolRegistry::new();
		for tool in ["code_moniker_read", "code_moniker_refresh"] {
			let result = registry
				.render_problem(
					tool,
					&serde_json::json!({"format": "json", "budget": "small"}),
					"code+moniker://",
					Some("test-runtime"),
					"workspace_busy",
				)
				.expect("known tool problem");
			assert!(result.is_error, "{tool}");
			assert!(result.text.is_empty(), "{tool}: {}", result.text);
			let structured = result.structured_content.expect("structured problem");
			assert_eq!(structured["tool"], tool);
			assert_eq!(structured["problem"], "workspace_busy");
		}
	}
}
