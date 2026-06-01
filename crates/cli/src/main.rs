use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;

use code_moniker_cli::Cli;

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
	let mut stdout = io::stdout();
	let mut stderr = io::stderr();
	let exit = code_moniker_cli::run(&cli, &mut stdout, &mut stderr);
	let _ = stdout.flush();
	let _ = stderr.flush();
	exit.into()
}
