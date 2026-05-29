pub(super) mod read;
pub(super) mod rules;
pub(in crate::mcp) mod scope;
pub(in crate::mcp) mod symbols;
pub(super) mod workspace;

use serde_json::Value;

use super::context::McpContext;
use read::ReadTool;
use rules::RulesTool;
use symbols::SymbolsTool;

pub(super) struct ToolDescriptor {
	pub(super) name: &'static str,
	pub(super) description: &'static str,
	pub(super) input_schema: Value,
}

impl ToolDescriptor {
	fn into_mcp_value(self) -> Value {
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
	rules: RulesTool,
	symbols: SymbolsTool,
}

impl ToolRegistry {
	pub(super) fn new() -> Self {
		Self {
			read: ReadTool,
			rules: RulesTool,
			symbols: SymbolsTool,
		}
	}

	pub(super) fn descriptors(&self) -> Vec<Value> {
		vec![
			self.read.descriptor().into_mcp_value(),
			self.symbols.descriptor().into_mcp_value(),
			self.rules.descriptor().into_mcp_value(),
		]
	}

	pub(super) fn call(
		&self,
		context: &McpContext,
		name: &str,
		arguments: &Value,
	) -> Result<ToolResult, ToolError> {
		match name {
			ReadTool::NAME => self.read.call(context, arguments),
			SymbolsTool::NAME => self.symbols.call(context, arguments),
			RulesTool::NAME => self.rules.call(context, arguments),
			_ => Err(ToolError::unknown_tool(name)),
		}
	}
}
