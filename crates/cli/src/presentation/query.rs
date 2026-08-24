use serde::Serialize;

use super::TemplateOutput;

const MCP_TEMPLATE: &str = include_str!("../../templates/query/mcp.md.j2");

pub(crate) fn mcp<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("query/mcp.md.j2", MCP_TEMPLATE, context)
}
