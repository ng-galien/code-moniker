use serde::Serialize;

use super::TemplateOutput;

const USAGES_TEMPLATE: &str = include_str!("../../templates/relationships/usages.md.j2");
const GRAPH_TEMPLATE: &str = include_str!("../../templates/relationships/graph.md.j2");
const DIFF_TEMPLATE: &str = include_str!("../../templates/relationships/diff.md.j2");

pub(crate) fn usages<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("relationships/usages.md.j2", USAGES_TEMPLATE, context)
}

pub(crate) fn graph<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("relationships/graph.md.j2", GRAPH_TEMPLATE, context)
}

pub(crate) fn diff<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("relationships/diff.md.j2", DIFF_TEMPLATE, context)
}
