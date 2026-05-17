use crate::ui::clipboard::ClipboardResult;
use crate::ui::events::Msg;
use crate::ui::live::StoreEvent;
use crate::ui::runtime::{TaskId, TaskResult, WorkKind};

#[derive(Debug)]
pub(in crate::ui) enum AppAction {
	Ui(Msg),
	Store(StoreEvent),
	TaskStarted {
		id: TaskId,
		work: WorkKind,
		generation: u64,
	},
	TaskCompleted(TaskResult),
	Clipboard(ClipboardResult),
}
