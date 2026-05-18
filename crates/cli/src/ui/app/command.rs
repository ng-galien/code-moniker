use crate::ui::store::navigation::NavigationAction;

#[derive(Debug)]
pub(in crate::ui) enum AppCommand {
	ApplyHeaderSearch {
		generation: Option<u64>,
		return_focus: bool,
	},
	CycleHeaderSearchSelector {
		direction: i8,
	},
	ToggleHeaderSearchSelection,
	FocusUsages,
	ToggleChangeMode,
	CopyPanelSnapshot,
	RunCheck,
	Navigation(Box<NavigationAction>),
	ToggleFocusRegion,
	PanelMove {
		direction: i8,
	},
	PanelHome,
	PanelEnd,
	ToggleSelectedNode,
	OpenSelectedNode,
	CloseNodeOrClearScope,
}
