use super::{FeatureId, NavItem, Route};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(in crate::ui) struct CommandId(String);

impl CommandId {
	pub(in crate::ui) fn new(value: impl Into<String>) -> Self {
		Self(value.into())
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct CommandSpec {
	pub(in crate::ui) id: CommandId,
	pub(in crate::ui) label: String,
	pub(in crate::ui) shortcut: Option<String>,
}

impl CommandSpec {
	pub(in crate::ui) fn new(
		id: CommandId,
		label: impl Into<String>,
		shortcut: Option<String>,
	) -> Self {
		Self {
			id,
			label: label.into(),
			shortcut,
		}
	}
}

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::ui) struct FeatureContext;

pub(in crate::ui) trait Feature: Send + Sync {
	fn id(&self) -> FeatureId;
	fn navigation(&self) -> Vec<NavItem>;

	fn commands(&self) -> Vec<CommandSpec> {
		Vec::new()
	}

	fn can_open(&self, route: &Route, _ctx: &FeatureContext) -> bool {
		route.feature == self.id()
	}
}
