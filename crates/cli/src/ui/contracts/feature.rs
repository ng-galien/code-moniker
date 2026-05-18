use super::{FeatureId, NavItem, Route};

pub(in crate::ui) trait Feature: Send + Sync {
	fn id(&self) -> FeatureId;
	fn navigation(&self) -> Vec<NavItem>;

	fn can_open(&self, route: &Route) -> bool {
		route.feature == self.id()
	}
}
