use std::io::Write;

use crate::{Exit, UiArgs, ui};

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
	_stderr: &mut W2,
) -> anyhow::Result<()> {
	let session = ui::boot(args);
	ui::run_session(session, stdout)
}
