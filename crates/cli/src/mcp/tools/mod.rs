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

impl ToolDescriptor {
	#[cfg(test)]
	fn into_mcp_value(mut self) -> Value {
		if supports_output_budget(self.name) {
			common::add_output_budget_schema(&mut self.input_schema);
		}
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
}

pub(super) trait McpTool {
	fn descriptor(&self) -> ToolDescriptor;
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
			.map(|tool| tool.descriptor().into_mcp_value())
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
			.map(|tool| tool.descriptor().into_rmcp_tool())
			.collect()
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
		if supports_output_budget(name) {
			common::validate_output_budget(arguments).map_err(ToolError::failed)?;
		}
		let mut result = tool.call(context, arguments)?;
		if !result.is_error && supports_output_budget(name) {
			result.text =
				common::apply_output_budget(result.text, arguments).map_err(ToolError::failed)?;
		}
		Ok(result)
	}
}

impl ToolDescriptor {
	fn into_rmcp_tool(mut self) -> Tool {
		if supports_output_budget(self.name) {
			common::add_output_budget_schema(&mut self.input_schema);
		}
		Tool::new(
			self.name,
			self.description,
			json_object_schema(self.input_schema),
		)
	}
}

fn supports_output_budget(name: &str) -> bool {
	name != RefreshTool::NAME
}

fn json_object_schema(schema: Value) -> JsonObject {
	match schema {
		Value::Object(object) => object,
		_ => JsonObject::new(),
	}
}
