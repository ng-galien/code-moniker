use crate::ui::contracts::{
	CommandId, CommandSpec, Feature, FeatureContext, FeatureId, NavItem, Route,
};

pub(in crate::ui) const FEATURE_ID: &str = "explorer";
pub(in crate::ui) const ROUTE_OVERVIEW: &str = "overview";
pub(in crate::ui) const ROUTE_OUTLINE: &str = "outline";
pub(in crate::ui) const ROUTE_REFS: &str = "refs";
pub(in crate::ui) const ROUTE_CHECK: &str = "check";

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::ui) struct ExplorerFeature;

impl ExplorerFeature {
	pub(in crate::ui) fn route(path: impl Into<String>) -> Route {
		Route::new(FEATURE_ID, path)
	}

	pub(in crate::ui) fn initial_route() -> Route {
		Self::route(ROUTE_OVERVIEW)
	}
}

impl Feature for ExplorerFeature {
	fn id(&self) -> FeatureId {
		FeatureId::new(FEATURE_ID)
	}

	fn navigation(&self) -> Vec<NavItem> {
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
		]
	}

	fn commands(&self) -> Vec<CommandSpec> {
		vec![
			CommandSpec::new(
				CommandId::new("explorer.filter"),
				"Edit filter",
				Some("/".into()),
			),
			CommandSpec::new(
				CommandId::new("explorer.usages"),
				"Focus usages",
				Some("u".into()),
			),
			CommandSpec::new(
				CommandId::new("explorer.check"),
				"Run checks",
				Some("c".into()),
			),
		]
	}

	fn can_open(&self, route: &Route, _ctx: &FeatureContext) -> bool {
		route.feature.as_str() == FEATURE_ID
			&& matches!(
				route.path.as_str(),
				ROUTE_OVERVIEW | ROUTE_OUTLINE | ROUTE_REFS | ROUTE_CHECK
			)
	}
}
