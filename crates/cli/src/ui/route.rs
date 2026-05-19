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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct Route {
	pub(in crate::ui) feature: FeatureId,
	pub(in crate::ui) path: String,
}

impl Route {
	pub(in crate::ui) fn new(feature: impl Into<String>, path: impl Into<String>) -> Self {
		Self {
			feature: FeatureId::new(feature),
			path: path.into(),
		}
	}
}
