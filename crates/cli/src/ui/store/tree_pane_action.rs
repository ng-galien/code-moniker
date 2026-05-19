#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum TreePaneAction {
	MoveDown,
	MoveUp,
	Home,
	End,
	ToggleSelected,
	OpenSelected,
	CloseSelected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum TreePaneNotice {
	Opened(String),
	Closed(String),
	MovedToParent,
	Noop,
}
