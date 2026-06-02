use code_moniker_core::lang::Lang;

use crate::ui::app::HeaderKindFilter;
use crate::ui::app::state::{
	ActiveFilter, ChangePanelMode, CheckState, FocusRegion, PanelNavigationState, PanelPolicy, View,
};
use crate::ui::async_task::{TaskId, TaskResult, WorkKind};
use crate::ui::clipboard::ClipboardResult;
use crate::ui::events::{HeaderSearchFocus, Msg};
use crate::ui::explorer::HeaderSearchResults;
use crate::ui::live::StoreEvent;
use crate::ui::workspace_read::UsageFocus;

#[derive(Debug)]
pub(in crate::ui) enum AppAction {
	Ui(Msg),
	HeaderSearchDebounced(u64),
	UsageLensDebounced(u64),
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
	SetView {
		view: View,
		policy: PanelPolicy,
	},
	ApplyHeaderSearch {
		results: HeaderSearchResults,
		return_focus: bool,
	},
	SetHeaderSearchFilters {
		langs: Vec<Lang>,
		kind_filters: Vec<HeaderKindFilter>,
	},
	SetHeaderSearchOptions {
		langs: Vec<Lang>,
		kind_filters: Vec<HeaderKindFilter>,
		available_langs: Vec<Lang>,
		available_kind_filters: Vec<HeaderKindFilter>,
		lang_cursor: usize,
		kind_cursor: usize,
	},
	SetHeaderSearchCursor {
		focus: HeaderSearchFocus,
		cursor: usize,
	},
	ClearFilter {
		return_focus: bool,
	},
	SetUsageLens(Option<UsageFocus>),
	ReplaceUsageLens(UsageFocus),
	EnterChangeMode,
	ReplaceActiveFilter(ActiveFilter),
	SetChangePanel(ChangePanelMode),
	SetFocusRegion(FocusRegion),
	ToggleViewsShowAll,
	SetPanelScroll(usize),
	SetPanelNavigation(PanelNavigationState),
}
