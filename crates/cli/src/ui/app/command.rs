use crate::ui::store::navigation::NavigationAction;

#[derive(Debug)]
pub(in crate::ui) enum AppCommand {
	ApplyFilter,
	ApplySearch,
	ClearFilter,
	FocusUsages,
	ToggleChangeMode,
	CopyPanelSnapshot,
	RunCheck,
	Navigation(NavigationAction),
	ToggleSelectedNode,
	OpenSelectedNode,
	CloseNodeOrClearScope,
}
