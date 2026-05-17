use crate::ui::app::state::{ActiveFilter, ChangePanelMode, CheckState, PanelPolicy, View};
use crate::ui::clipboard::ClipboardResult;
use crate::ui::contracts::Route;
use crate::ui::events::Msg;
use crate::ui::live::StoreEvent;
use crate::ui::runtime::{TaskId, TaskResult, WorkKind};
use crate::workspace::UsageFocus;

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
	ApplyFilter(ActiveFilter),
	ClearFilter,
	FocusUsages(UsageFocus),
	EnterChangeMode,
	ReplaceActiveFilter(ActiveFilter),
	SetChangePanel(ChangePanelMode),
	#[cfg(test)]
	EmitNotify(String),
}
