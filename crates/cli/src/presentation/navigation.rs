use serde::Serialize;

use super::TemplateOutput;

const READ_AST_TEMPLATE: &str = include_str!("../../templates/navigation/read-ast.md.j2");
const READ_EXPLORER_TEMPLATE: &str = include_str!("../../templates/navigation/read-explorer.md.j2");
const READ_SYMBOL_TEMPLATE: &str = include_str!("../../templates/navigation/read-symbol.md.j2");
const READ_VIEW_LIST_TEMPLATE: &str =
	include_str!("../../templates/navigation/read-view-list.md.j2");
const READ_VIEW_DETAIL_TEMPLATE: &str =
	include_str!("../../templates/navigation/read-view-detail.md.j2");
const CONTEXT_TEMPLATE: &str = include_str!("../../templates/navigation/context.md.j2");

pub(crate) fn read_ast<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("navigation/read-ast.md.j2", READ_AST_TEMPLATE, context)
}

pub(crate) fn read_explorer<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new(
		"navigation/read-explorer.md.j2",
		READ_EXPLORER_TEMPLATE,
		context,
	)
}

pub(crate) fn read_symbol<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new(
		"navigation/read-symbol.md.j2",
		READ_SYMBOL_TEMPLATE,
		context,
	)
}

pub(crate) fn read_view_list<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new(
		"navigation/read-view-list.md.j2",
		READ_VIEW_LIST_TEMPLATE,
		context,
	)
}

pub(crate) fn read_view_detail<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new(
		"navigation/read-view-detail.md.j2",
		READ_VIEW_DETAIL_TEMPLATE,
		context,
	)
}

pub(crate) fn context<T: Serialize>(context: &T) -> anyhow::Result<TemplateOutput> {
	TemplateOutput::new("navigation/context.md.j2", CONTEXT_TEMPLATE, context)
}
