use serde::Serialize;

use super::TemplateOutput;

const SEARCH_TEMPLATE: &str = include_str!("../../templates/symbols/search.md.j2");
const LIST_TEMPLATE: &str = include_str!("../../templates/symbols/list.md.j2");
const INSIGHTS_TEMPLATE: &str = include_str!("../../templates/symbols/insights.md.j2");

pub(crate) fn search<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("symbols/search.md.j2", SEARCH_TEMPLATE, context)
}

pub(crate) fn list<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("symbols/list.md.j2", LIST_TEMPLATE, context)
}

pub(crate) fn insights<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("symbols/insights.md.j2", INSIGHTS_TEMPLATE, context)
}
