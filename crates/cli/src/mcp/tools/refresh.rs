use code_moniker_query::{Command, CommandRequest, CommandResponse, WorkspaceStatus};
use serde::Serialize;
use serde_json::{Value, json};

use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

pub(in crate::mcp) struct RefreshTool;

#[derive(Serialize)]
struct RefreshResult {
	uri: &'static str,
	completeness: &'static str,
	generation: Option<u64>,
	files: usize,
	symbols: usize,
	references: usize,
	stale: bool,
	message: String,
}

impl RefreshResult {
	fn from_response(response: CommandResponse) -> Result<Self, ToolError> {
		let CommandResponse {
			generation,
			message,
			status,
		} = response;
		let WorkspaceStatus {
			files,
			symbols,
			references,
			stale,
			..
		} = *status.ok_or_else(|| {
			ToolError::failed("daemon refresh response did not include workspace status")
		})?;
		Ok(Self {
			uri: "workspace",
			completeness: "full",
			generation: generation.map(|generation| generation.0),
			files,
			symbols,
			references,
			stale,
			message,
		})
	}

	fn into_tool_result(self) -> Result<ToolResult, ToolError> {
		let generation = self
			.generation
			.map(|generation| generation.to_string())
			.unwrap_or_else(|| "<unknown>".to_string());
		let text = format!(
			"uri: {}\ncompleteness: {}\n\nrefreshed: generation {generation}\nfiles: {}\ndefs: {}\nrefs: {}\nstale: {}\n{}\n",
			self.uri,
			self.completeness,
			self.files,
			self.symbols,
			self.references,
			if self.stale { "stale" } else { "fresh" },
			self.message
		);
		let structured_content = serde_json::to_value(&self).map_err(ToolError::failed)?;
		Ok(ToolResult::success(text).with_structured_content(structured_content))
	}
}

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

	fn output_contract(&self) -> OutputContract {
		OutputContract::Plain
	}

	fn call(
		&self,
		context: &McpContext,
		_arguments: &Value,
		_output: OutputOptions,
	) -> Result<ToolResult, ToolError> {
		let response = context
			.command(CommandRequest {
				command: Command::WorkspaceRefresh,
			})
			.map_err(ToolError::failed)?;
		RefreshResult::from_response(response)?.into_tool_result()
	}
}
