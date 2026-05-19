mod panel_content;
mod search;
mod vm;

use crate::ui::app::App;
use crate::ui::panel::PanelVm;

pub(in crate::ui) use panel_content::ActivePanelNav;
pub(in crate::ui) use search::{HeaderSearchResults, header_search_options, header_search_results};
pub(in crate::ui) use vm::{
	ExplorerVm, FooterVm, HeaderVm, NavPaneVm, NavRowVm, NavRowVmKind, SearchBarVm, SearchPopupVm,
};

pub(in crate::ui) fn active_panel(app: &App) -> PanelVm {
	panel_content::active_panel(app)
}

pub(in crate::ui) fn active_panel_nav(app: &App) -> ActivePanelNav {
	panel_content::active_panel_nav(app)
}

pub(in crate::ui) fn active_panel_tree_rows(app: &App) -> Vec<crate::ui::render::tree::TreeRowVm> {
	panel_content::active_panel_tree_rows(app)
}

pub(in crate::ui) fn active_panel_tree_rows_with_expanded(
	app: &App,
	expanded: &std::collections::BTreeSet<String>,
) -> Vec<crate::ui::render::tree::TreeRowVm> {
	panel_content::active_panel_tree_rows_with_expanded(app, expanded)
}

pub(in crate::ui) fn active_panel_default_expanded(
	app: &App,
) -> std::collections::BTreeSet<String> {
	panel_content::active_panel_default_expanded(app)
}

pub(in crate::ui) fn view_model(app: &App) -> ExplorerVm {
	ExplorerVm::from_app(app)
}
