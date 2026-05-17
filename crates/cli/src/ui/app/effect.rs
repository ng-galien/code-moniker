use crate::ui::contracts::Route;
use crate::ui::runtime::TaskSpec;

use super::AppCommand;

#[derive(Debug)]
#[allow(dead_code)]
pub(in crate::ui) enum Effect {
	Navigate(Route),
	Back,
	Quit,
	Notify(String),
	Refresh,
	Spawn(TaskSpec),
	RunCommand(AppCommand),
	None,
}
