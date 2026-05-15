use crate::ui::contracts::{CommandSpec, Feature, FeatureContext, NavItem, Route};
use crate::ui::features::explorer::ExplorerFeature;

pub(in crate::ui) struct FeatureRegistry {
	features: Vec<Box<dyn Feature>>,
}

impl FeatureRegistry {
	pub(in crate::ui) fn static_registry() -> Self {
		Self {
			features: vec![Box::new(ExplorerFeature)],
		}
	}

	pub(in crate::ui) fn initial_route(&self) -> Route {
		ExplorerFeature::initial_route()
	}

	pub(in crate::ui) fn can_open(&self, route: &Route) -> bool {
		let ctx = FeatureContext;
		self.features
			.iter()
			.any(|feature| feature.can_open(route, &ctx))
	}

	pub(in crate::ui) fn navigation(&self) -> Vec<NavItem> {
		let mut items: Vec<NavItem> = self
			.features
			.iter()
			.flat_map(|feature| feature.navigation())
			.collect();
		items.sort_by(|a, b| {
			a.order
				.cmp(&b.order)
				.then_with(|| a.label.cmp(&b.label))
				.then_with(|| a.route.path.cmp(&b.route.path))
		});
		items
	}

	pub(in crate::ui) fn commands(&self) -> Vec<CommandSpec> {
		self.features
			.iter()
			.flat_map(|feature| feature.commands())
			.collect()
	}
}
