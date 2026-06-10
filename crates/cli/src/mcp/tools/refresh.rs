use std::time::Duration;

use serde_json::{Value, json};

use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

const REFRESH_TIMEOUT: Duration = Duration::from_secs(120);

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
		let Some(live) = context.live() else {
			return Err(ToolError::failed(
				"live refresh is not available in this session",
			));
		};
		let outcome = live
			.request_refresh(REFRESH_TIMEOUT)
			.map_err(ToolError::failed)?;
		if let Some(error) = outcome.error {
			return Err(ToolError::failed(error));
		}
		let text = format!(
			"uri: workspace\ncompleteness: complete\n\nrefreshed: generation {}\nfiles {} defs {} refs {}\nstale: {}\n",
			outcome.generation,
			outcome.files,
			outcome.symbols,
			outcome.references,
			context.index().staleness().summary(),
		);
		Ok(ToolResult {
			text,
			is_error: false,
		})
	}
}
