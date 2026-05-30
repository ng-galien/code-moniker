use crate::Exit;
use crate::args::UiArgs;
use std::io::Write;

#[cfg(test)]
mod acceptance;
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
mod workspace_read;

const DEFAULT_PANEL_SNAPSHOT_WIDTH: usize = 100;

pub(crate) use shell::terminal::UiSession;

pub(crate) fn boot(args: &UiArgs) -> UiSession {
	shell::terminal::boot(args)
}

pub(crate) fn run_session<W: Write>(session: UiSession, stdout: &mut W) -> anyhow::Result<()> {
	shell::terminal::run_session(stdout, session)
}

pub fn run<W1: Write, W2: Write>(args: &UiArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	shell::terminal::run(args, stdout, stderr)
}
