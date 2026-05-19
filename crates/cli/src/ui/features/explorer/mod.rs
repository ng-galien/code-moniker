mod panels;
mod search;
mod vm;

use crate::ui::App;
use crate::ui::contracts::{NavItem, Route};
use crate::ui::panels::PanelVm;

pub(in crate::ui) use search::{HeaderSearchResults, header_search_options, header_search_results};
pub(in crate::ui) use vm::{
	ExplorerVm, FooterVm, HeaderVm, NavPaneVm, NavRowVm, NavRowVmKind, SearchBarVm, SearchPopupVm,
};

pub(in crate::ui) const FEATURE_ID: &str = "explorer";
pub(in crate::ui) const ROUTE_OVERVIEW: &str = "overview";
pub(in crate::ui) const ROUTE_OUTLINE: &str = "outline";
pub(in crate::ui) const ROUTE_REFS: &str = "refs";
pub(in crate::ui) const ROUTE_CHECK: &str = "check";
pub(in crate::ui) const ROUTE_CHANGE: &str = "change";

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::ui) struct ExplorerFeature;

impl ExplorerFeature {
	pub(in crate::ui) fn route(path: impl Into<String>) -> Route {
		Route::new(FEATURE_ID, path)
	}

	pub(in crate::ui) fn initial_route() -> Route {
		Self::route(ROUTE_OVERVIEW)
	}

	pub(in crate::ui) fn active_panel(app: &App) -> PanelVm {
		panels::active_panel(app)
	}

	pub(in crate::ui) fn view_model(app: &App) -> ExplorerVm {
		ExplorerVm::from_app(app)
	}

	pub(in crate::ui) fn navigation() -> Vec<NavItem> {
		vec![
			NavItem::new(
				"Overview",
				Self::route(ROUTE_OVERVIEW),
				Some("Explorer".into()),
				10,
			),
			NavItem::new(
				"Outline",
				Self::route(ROUTE_OUTLINE),
				Some("Explorer".into()),
				20,
			),
			NavItem::new("Refs", Self::route(ROUTE_REFS), Some("Explorer".into()), 30),
			NavItem::new(
				"Check",
				Self::route(ROUTE_CHECK),
				Some("Explorer".into()),
				40,
			),
			NavItem::new(
				"Change",
				Self::route(ROUTE_CHANGE),
				Some("Explorer".into()),
				50,
			),
		]
	}

	pub(in crate::ui) fn can_open(route: &Route) -> bool {
		route.feature.as_str() == FEATURE_ID
			&& matches!(
				route.path.as_str(),
				ROUTE_OVERVIEW | ROUTE_OUTLINE | ROUTE_REFS | ROUTE_CHECK | ROUTE_CHANGE
			)
	}
}
