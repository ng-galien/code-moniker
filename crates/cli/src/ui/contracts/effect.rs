use super::Route;
use crate::ui::runtime::TaskSpec;

#[derive(Debug)]
#[allow(dead_code)]
pub(in crate::ui) enum Effect {
	Navigate(Route),
	Back,
	Quit,
	Notify(String),
	Refresh,
	Spawn(TaskSpec),
	None,
}
