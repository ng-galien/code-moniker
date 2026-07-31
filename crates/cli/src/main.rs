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
	let exit = code_moniker_cli::run(&cli, &mut stdout, &mut stderr);
	#[cfg(any(feature = "mcp", feature = "telemetry"))]
	drop(telemetry);
	let _ = stdout.flush();
	let _ = stderr.flush();
	exit.into()
}
