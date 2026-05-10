use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;

use pg_code_moniker::cli::{self, Cli};

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
	let mut stdout = io::stdout().lock();
	let mut stderr = io::stderr().lock();
	let exit = cli::run(&cli, &mut stdout, &mut stderr);
	let _ = stdout.flush();
	let _ = stderr.flush();
	exit.into()
}
