use std::io::Write;

use crate::{Exit, UiArgs, mcp, ui};

pub(crate) fn run<W1: Write, W2: Write>(args: &UiArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match run_inner(args, stdout, stderr) {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn run_inner<W1: Write, W2: Write>(
	args: &UiArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> anyhow::Result<()> {
	let session = ui::boot(args);
	let _mcp = start_mcp_if_requested(args, &session, stderr)?;
	ui::run_session(session, stdout)
}

fn start_mcp_if_requested<W: Write>(
	args: &UiArgs,
	session: &ui::UiSession,
	stderr: &mut W,
) -> anyhow::Result<Option<mcp::McpServer>> {
	if !args.mcp {
		return Ok(None);
	}
	let server = mcp::start(
		session.options().clone(),
		session.scheme().to_string(),
		args.mcp_port,
		session.shared_workspace_index(),
	)?;
	writeln!(stderr, "code-moniker mcp: {}", server.endpoint())?;
	Ok(Some(server))
}
