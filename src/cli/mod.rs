//! Standalone CLI surface. See `docs/CLI.md`.

pub mod args;
pub mod extract;
pub mod format;
pub mod lang;
pub mod predicate;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

pub use args::{Args, OutputFormat, OutputMode};
pub use lang::{LangError, path_to_lang};
pub use predicate::{MatchSet, Predicate};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Exit {
	Match,
	NoMatch,
	UsageError,
}

impl From<Exit> for ExitCode {
	fn from(e: Exit) -> Self {
		match e {
			Exit::Match => ExitCode::SUCCESS,
			Exit::NoMatch => ExitCode::from(1),
			Exit::UsageError => ExitCode::from(2),
		}
	}
}

pub fn run<W1: Write, W2: Write>(args: &Args, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match run_inner(args, stdout) {
		Ok(any) => {
			if any {
				Exit::Match
			} else {
				Exit::NoMatch
			}
		}
		Err(e) => {
			let _ = writeln!(stderr, "pg-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn run_inner<W: Write>(args: &Args, stdout: &mut W) -> anyhow::Result<bool> {
	let path: &Path = args.file.as_ref();
	let lang = path_to_lang(path)?;
	let source = std::fs::read_to_string(path)
		.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
	let scheme = args
		.scheme
		.clone()
		.unwrap_or_else(|| format!("{}+moniker://", lang.tag()));
	let predicates = args.compiled_predicates(&scheme)?;
	let graph = extract::extract(lang, &source);
	let matches = predicate::filter(&graph, &predicates, &args.kind);
	let any = !matches.defs.is_empty() || !matches.refs.is_empty();
	match args.mode() {
		OutputMode::Default => match args.format {
			OutputFormat::Tsv => format::write_tsv(stdout, &matches, &source, args, &scheme)?,
			OutputFormat::Json => {
				format::write_json(stdout, &matches, &source, args, lang, path, &scheme)?
			}
		},
		OutputMode::Count => {
			let n = matches.defs.len() + matches.refs.len();
			writeln!(stdout, "{n}")?;
		}
		OutputMode::Quiet => {}
	}
	Ok(any)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn exit_codes_are_stable() {
		assert_eq!(ExitCode::from(Exit::Match), ExitCode::SUCCESS);
		assert_eq!(ExitCode::from(Exit::NoMatch), ExitCode::from(1));
		assert_eq!(ExitCode::from(Exit::UsageError), ExitCode::from(2));
	}
}
