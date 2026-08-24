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
		if self == Self::Agent {
			common::add_compact_output_schema(schema);
			common::add_output_budget_schema(schema);
		}
	}

	fn output_options(self, arguments: &Value) -> Result<OutputOptions, ToolError> {
		match self {
			Self::Agent => common::AgentOutputOptions::from_arguments(arguments)
				.map(OutputOptions::agent)
				.map_err(ToolError::failed),
			Self::Plain => Ok(OutputOptions::plain()),
		}
	}

	fn finalize(
		self,
		mut result: ToolResult,
		options: OutputOptions,
		scheme: &str,
		runtime: Option<&str>,
	) -> Result<ToolResult, ToolError> {
		if self == Self::Plain {
			return Ok(result);
		}
		let agent = options.agent_options();
		let template = result.template.take().ok_or_else(|| {
			ToolError::failed("agent output must be produced by the shared template renderer")
		})?;
		result.text = TemplateOutput::render(
			&template,
			RenderOptions {
				compact: agent.compact,
				scheme,
				runtime,
			},
		)
		.map_err(ToolError::failed)?;
		Ok(result)
	}
}

#[derive(Clone, Copy, Debug)]
pub(super) struct OutputOptions {
	agent: Option<common::AgentOutputOptions>,
}

impl OutputOptions {
	fn agent(options: common::AgentOutputOptions) -> Self {
		Self {
			agent: Some(options),
		}
	}

	fn plain() -> Self {
		Self { agent: None }
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
	template: Option<TemplateOutput>,
}

impl ToolResult {
	pub(super) fn success(text: impl Into<String>) -> Self {
		Self {
			text: text.into(),
			is_error: false,
			template: None,
		}
	}

	pub(super) fn templated(template: TemplateOutput) -> Self {
		Self {
			text: String::new(),
			is_error: false,
			template: Some(template),
		}
	}

	fn templated_error(template: TemplateOutput) -> Self {
		Self {
			text: String::new(),
			is_error: true,
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
		if contract != OutputContract::Agent {
			return None;
		}
		let options = contract
			.output_options(arguments)
			.or_else(|_| contract.output_options(&serde_json::json!({})))
			.ok()?;
		let result = agent_problem_result(name, arguments, message).ok()?;
		contract.finalize(result, options, scheme, runtime).ok()
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

fn agent_problem_result(
	tool: &str,
	arguments: &Value,
	message: &str,
) -> Result<ToolResult, ToolError> {
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
	let view = McpProblemView {
		uri,
		tool,
		problem,
		fix_hint,
	};
	problem_presentation::mcp(&view)
		.map(ToolResult::templated_error)
		.map_err(ToolError::failed)
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
	use super::{OutputContract, ToolResult, agent_problem_result};

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
}
