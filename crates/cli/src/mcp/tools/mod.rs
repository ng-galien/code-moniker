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

use serde_json::Value;

use super::context::McpContext;
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

	fn validate_arguments(self, arguments: &Value) -> Result<(), ToolError> {
		if self == Self::Agent {
			common::compact_argument(arguments).map_err(ToolError::failed)?;
			common::validate_output_budget(arguments).map_err(ToolError::failed)?;
		}
		Ok(())
	}

	fn finalize(
		self,
		mut result: ToolResult,
		arguments: &Value,
		scheme: &str,
	) -> Result<ToolResult, ToolError> {
		if self == Self::Plain {
			return Ok(result);
		}
		let compact = match common::compact_argument(arguments) {
			Ok(compact) => compact,
			Err(_) if result.is_error => true,
			Err(error) => return Err(ToolError::failed(error)),
		};
		result.text = common::compact_response_monikers(
			result.text,
			compact,
			scheme,
			result.monikers.iter().map(String::as_str),
		);
		result.text = match common::apply_output_budget(result.text.clone(), arguments) {
			Ok(text) => text,
			Err(_) if result.is_error => {
				common::apply_output_budget(result.text.clone(), &serde_json::json!({}))
					.unwrap_or(result.text)
			}
			Err(error) => return Err(ToolError::failed(error)),
		};
		Ok(result)
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
	monikers: Vec<String>,
}

impl ToolResult {
	pub(super) fn success(text: impl Into<String>) -> Self {
		Self {
			text: text.into(),
			is_error: false,
			monikers: Vec::new(),
		}
	}

	pub(super) fn error(text: impl Into<String>) -> Self {
		Self {
			text: text.into(),
			is_error: true,
			monikers: Vec::new(),
		}
	}

	pub(super) fn with_monikers<'a>(mut self, monikers: impl IntoIterator<Item = &'a str>) -> Self {
		self.monikers
			.extend(monikers.into_iter().map(str::to_owned));
		self
	}
}

pub(super) trait McpTool {
	fn descriptor(&self) -> ToolDescriptor;
	fn output_contract(&self) -> OutputContract;
	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError>;
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

	pub(super) fn finalize_error(
		&self,
		name: &str,
		arguments: &Value,
		scheme: &str,
		text: String,
	) -> Option<ToolResult> {
		let contract = self.contract_for_tool(name)?;
		let mut result = ToolResult::error(text);
		if let Some(uri) = arguments.get("uri").and_then(Value::as_str)
			&& uri.contains("+moniker://")
		{
			result = result.with_monikers([uri]);
		}
		OutputContract::finalize(contract, result, arguments, scheme).ok()
	}

	fn contract_for_tool(&self, name: &str) -> Option<OutputContract> {
		self.all()
			.into_iter()
			.find(|tool| tool.descriptor().name == name)
			.map(|tool| tool.output_contract())
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
		let contract = tool.output_contract();
		contract.validate_arguments(arguments)?;
		let mut result = tool.call(context, arguments)?;
		if contract == OutputContract::Agent {
			result.text = format!("runtime: {}\n{}", context.runtime_label(), result.text);
		}
		OutputContract::finalize(contract, result, arguments, context.scheme())
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
	use super::{OutputContract, ToolResult};

	#[test]
	fn agent_contract_compacts_before_applying_the_hard_budget() {
		let canonical = concat!(
			"code+moniker://./lang:rs/dir:crates/dir:cli/dir:src/module:mcp/",
			"module:tools/enum:OutputContract/method:finalize(result:ToolResult,arguments:&Value)"
		);
		let text = std::iter::repeat_n(canonical, 10)
			.collect::<Vec<_>>()
			.join("\n");
		assert!(text.chars().count() > 1_000);

		let result = OutputContract::finalize(
			OutputContract::Agent,
			ToolResult::success(text).with_monikers([canonical]),
			&serde_json::json!({"max_chars": 1000}),
			"code+moniker://",
		)
		.expect("agent output contract");

		assert!(!result.text.contains("code+moniker://"));
		assert!(!result.text.contains("output omitted by budget"));
		assert!(result.text.chars().count() < 1_000);
	}

	#[test]
	fn agent_contract_compacts_and_bounds_error_results() {
		let canonical = "code+moniker://./lang:rs/module:mcp/struct:Missing";
		let text = std::iter::repeat_n(canonical, 100)
			.collect::<Vec<_>>()
			.join("\n");

		let result = OutputContract::finalize(
			OutputContract::Agent,
			ToolResult::error(text).with_monikers([canonical]),
			&serde_json::json!({"max_chars": 1000}),
			"code+moniker://",
		)
		.expect("agent error output contract");

		assert!(result.is_error);
		assert!(!result.text.contains("code+moniker://"));
		assert!(result.text.chars().count() <= 1_000);
		assert!(result.text.contains("truncated_by: max_chars"));
	}
}
