use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;

use code_moniker_cli::Cli;

#[cfg(any(feature = "mcp", feature = "telemetry"))]
mod observability;

fn main() -> ExitCode {
	let cli = match Cli::try_parse() {
		Ok(c) => c,
		Err(e) => {
			let _ = e.print();
			return match e.kind() {
				clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
					ExitCode::SUCCESS
				}
				_ => ExitCode::from(2),
			};
		}
	};
	#[cfg(any(feature = "mcp", feature = "telemetry"))]
	let telemetry = observability::init();
	let mut stdout = io::stdout();
	let mut stderr = io::stderr();
	#[cfg(any(feature = "mcp", feature = "telemetry"))]
	let command_span = observability::command_span(&cli);
	#[cfg(any(feature = "mcp", feature = "telemetry"))]
	let _command_entered = command_span.enter();
	let exit = code_moniker_cli::run(&cli, &mut stdout, &mut stderr);
	#[cfg(any(feature = "mcp", feature = "telemetry"))]
	command_span.record(
		"command.status",
		match exit {
			code_moniker_cli::Exit::Match => "ok",
			code_moniker_cli::Exit::NoMatch => "no_match",
			code_moniker_cli::Exit::UsageError => "usage_error",
		},
	);
	#[cfg(any(feature = "mcp", feature = "telemetry"))]
	drop(_command_entered);
	#[cfg(any(feature = "mcp", feature = "telemetry"))]
	drop(command_span);
	#[cfg(any(feature = "mcp", feature = "telemetry"))]
	drop(telemetry);
	let _ = stdout.flush();
	let _ = stderr.flush();
	exit.into()
}
