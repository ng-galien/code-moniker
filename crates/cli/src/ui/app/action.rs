use code_moniker_core::lang::Lang;

use crate::ui::app::state::{
	ActiveFilter, ChangePanelMode, CheckState, HeaderSearchResults, PanelPolicy, View,
};
use crate::ui::clipboard::ClipboardResult;
use crate::ui::contracts::Route;
use crate::ui::events::Msg;
use crate::ui::live::StoreEvent;
use crate::ui::runtime::{TaskId, TaskResult, WorkKind};
use crate::workspace::UsageFocus;

#[derive(Debug)]
pub(in crate::ui) enum AppAction {
	Ui(Msg),
	HeaderSearchDebounced(u64),
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
	ApplyHeaderSearch {
		results: HeaderSearchResults,
		return_focus: bool,
	},
	SetHeaderSearchFilters {
		lang: Option<Lang>,
		kind: Option<String>,
	},
	ClearFilter {
		return_focus: bool,
	},
	FocusUsages(UsageFocus),
	EnterChangeMode,
	ReplaceActiveFilter(ActiveFilter),
	SetChangePanel(ChangePanelMode),
	#[cfg(test)]
	EmitNotify(String),
}
