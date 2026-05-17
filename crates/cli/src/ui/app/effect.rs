use crate::ui::contracts::Route;
#[cfg(test)]
use crate::ui::runtime::TaskSpec;

use super::AppCommand;

#[derive(Debug)]
pub(in crate::ui) enum Effect {
	Navigate(Route),
	Quit,
	#[cfg(test)]
	Notify(String),
	#[cfg(test)]
	Spawn(TaskSpec),
	DebounceHeaderSearch(u64),
	RunCommand(AppCommand),
}
