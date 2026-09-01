use serde::Serialize;

use super::{TemplateOutput, render};

const SHOW_TEMPLATE: &str = include_str!("../../templates/rules/show.md.j2");
const MCP_LIST_TEMPLATE: &str = include_str!("../../templates/rules/mcp-list.md.j2");
const MCP_RUN_TEMPLATE: &str = include_str!("../../templates/rules/mcp-run.md.j2");
const LEARN_INDEX_TEMPLATE: &str = include_str!("../../templates/rules/learn-index.md.j2");
const LEARN_TOPIC_TEMPLATE: &str = include_str!("../../templates/rules/learn-topic.md.j2");

pub(crate) fn show<T: Serialize>(context: &T) -> anyhow::Result<String> {
	render("rules/show.md.j2", SHOW_TEMPLATE, context)
}

pub(crate) fn mcp_list<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("rules/mcp-list.md.j2", MCP_LIST_TEMPLATE, context)
}

pub(crate) fn mcp_run<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("rules/mcp-run.md.j2", MCP_RUN_TEMPLATE, context)
}

pub(crate) fn learn_index<T: Serialize>(context: &T) -> anyhow::Result<String> {
	render("rules/learn-index.md.j2", LEARN_INDEX_TEMPLATE, context)
}

pub(crate) fn learn_topic<T: Serialize>(context: &T) -> anyhow::Result<String> {
	render("rules/learn-topic.md.j2", LEARN_TOPIC_TEMPLATE, context)
}
