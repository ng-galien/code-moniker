use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(in crate::ui) struct FeatureId(String);

impl FeatureId {
	pub(in crate::ui) fn new(value: impl Into<String>) -> Self {
		Self(value.into())
	}

	pub(in crate::ui) fn as_str(&self) -> &str {
		&self.0
	}
}

impl fmt::Display for FeatureId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct RouteParams {
	values: BTreeMap<String, String>,
}

impl RouteParams {
	pub(in crate::ui) fn empty() -> Self {
		Self::default()
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct Route {
	pub(in crate::ui) feature: FeatureId,
	pub(in crate::ui) path: String,
	pub(in crate::ui) params: RouteParams,
}

impl Route {
	pub(in crate::ui) fn new(feature: impl Into<String>, path: impl Into<String>) -> Self {
		Self {
			feature: FeatureId::new(feature),
			path: path.into(),
			params: RouteParams::empty(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct NavItem {
	pub(in crate::ui) label: String,
	pub(in crate::ui) route: Route,
	pub(in crate::ui) group: Option<String>,
	pub(in crate::ui) order: i32,
}

impl NavItem {
	pub(in crate::ui) fn new(
		label: impl Into<String>,
		route: Route,
		group: Option<String>,
		order: i32,
	) -> Self {
		Self {
			label: label.into(),
			route,
			group,
			order,
		}
	}
}
