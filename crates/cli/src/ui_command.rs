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
	init_ui_logging();
	let session = ui::boot(args);
	ui::run_session(session, stdout)
}

fn init_ui_logging() {
	if let Some(log_path) = std::env::var_os("CODE_MONIKER_UI_LOG") {
		let path = std::path::PathBuf::from(log_path).with_extension("trace.log");
		if let Ok(file) = std::fs::OpenOptions::new()
			.create(true)
			.append(true)
			.open(path)
		{
			let _ = tracing_subscriber::fmt()
				.with_writer(std::sync::Mutex::new(file))
				.with_max_level(tracing::Level::DEBUG)
				.with_target(true)
				.with_level(true)
				.compact()
				.try_init();
		}
	}
}
