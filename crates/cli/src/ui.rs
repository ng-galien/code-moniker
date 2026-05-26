use crate::Exit;
use crate::args::UiArgs;
use std::io::Write;

mod app;
mod async_task;
mod clipboard;
mod events;
mod explorer;
mod live;
mod panel;
mod render;
mod shell;
mod store;
mod workspace_state;

const DEFAULT_PANEL_SNAPSHOT_WIDTH: usize = 100;
pub fn run<W1: Write, W2: Write>(args: &UiArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	shell::terminal::run(args, stdout, stderr)
}
