use code_moniker_query::{Command, CommandRequest};
use serde_json::{Value, json};

use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

pub(in crate::mcp) struct RefreshTool;

impl RefreshTool {
	pub(super) const NAME: &'static str = "code_moniker_refresh";

	const DESCRIPTION: &'static str = concat!(
		"When to use: whenever another code_moniker tool reports that the workspace index is stale. ",
		"Applies the pending file changes to the index and linkage (incremental when possible), ",
		"republishes the workspace snapshot, and reports the refreshed generation. No arguments."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {},
			"additionalProperties": false
		})
	}
}

impl McpTool for RefreshTool {
	fn descriptor(&self) -> ToolDescriptor {
		ToolDescriptor {
			name: Self::NAME,
			description: Self::DESCRIPTION,
			input_schema: Self::input_schema(),
		}
	}

	fn call(&self, context: &McpContext, _arguments: &Value) -> Result<ToolResult, ToolError> {
		let response = context
			.command(CommandRequest {
				command: Command::WorkspaceRefresh,
			})
			.map_err(ToolError::failed)?;
		let generation = response
			.generation
			.map(|generation| generation.0.to_string())
			.unwrap_or_else(|| "<unknown>".to_string());
		let status = response.status.as_ref().ok_or_else(|| {
			ToolError::failed("daemon refresh response did not include workspace status")
		})?;
		Ok(ToolResult {
			text: format!(
				"uri: workspace\ncompleteness: full\n\nrefreshed: generation {generation}\nfiles: {}\ndefs: {}\nrefs: {}\nstale: {}\n{}\n",
				status.files,
				status.symbols,
				status.references,
				if status.stale { "stale" } else { "fresh" },
				response.message
			),
			is_error: false,
		})
	}
}
