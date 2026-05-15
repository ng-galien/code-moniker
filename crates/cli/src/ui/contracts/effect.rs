use super::Route;

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(in crate::ui) enum Effect {
	Navigate(Route),
	Back,
	Quit,
	Notify(String),
	Refresh,
	Spawn(Task),
	None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct Task {
	pub(in crate::ui) id: String,
	pub(in crate::ui) label: String,
}

impl Task {
	#[allow(dead_code)]
	pub(in crate::ui) fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
		Self {
			id: id.into(),
			label: label.into(),
		}
	}
}
