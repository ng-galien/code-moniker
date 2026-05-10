use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;

use pg_code_moniker::cli::{self, Args};

fn main() -> ExitCode {
	let args = match Args::try_parse() {
		Ok(a) => a,
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
	let exit = cli::run(&args, &mut stdout, &mut stderr);
	let _ = stdout.flush();
	let _ = stderr.flush();
	exit.into()
}
