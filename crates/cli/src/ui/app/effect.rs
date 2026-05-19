//! Runtime commands interpreted by the shell/runtime boundary.

use crate::ui::app::View;

#[derive(Debug)]
pub(in crate::ui) enum Effect {
	ShowView(View),
	Quit,
	DebounceHeaderSearch(u64),
	CopyPanelSnapshot,
	RunCheck,
}
