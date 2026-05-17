use crate::ui::app::state::{
	ActiveFilter, ChangePanelMode, CheckState, PanelPolicy, View, VisualizationMode,
};
use crate::ui::clipboard::ClipboardResult;
use crate::ui::contracts::Route;
use crate::ui::events::{Msg, UiMode};
use crate::ui::live::StoreEvent;
use crate::ui::runtime::{TaskId, TaskResult, WorkKind};

#[derive(Debug)]
pub(in crate::ui) enum AppAction {
	Ui(Msg),
	Shell(ShellAction),
	Store(StoreEvent),
	TaskStarted {
		id: TaskId,
		work: WorkKind,
		generation: u64,
	},
	TaskCompleted(TaskResult),
	Clipboard(ClipboardResult),
}

#[derive(Debug)]
pub(in crate::ui) enum ShellAction {
	SetStatus(String),
	AppendStatus(String),
	SetCheckState(CheckState),
	SetRoute(Route),
	SetView {
		view: View,
		policy: PanelPolicy,
		route: Route,
	},
	SetActiveFilter {
		active_filter: ActiveFilter,
		view_mode: VisualizationMode,
		panel_policy: PanelPolicy,
		mode: UiMode,
		change_panel: Option<ChangePanelMode>,
		clear_filter_draft: bool,
		clear_search_draft: bool,
	},
	ReplaceActiveFilter(ActiveFilter),
	SetChangePanel(ChangePanelMode),
}
