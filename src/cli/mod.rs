//! Standalone CLI surface. See `docs/CLI.md`.

pub mod args;
pub mod check;
pub mod extract;
pub mod format;
pub mod lang;
pub mod lines;
pub mod predicate;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

pub use args::{Args, CheckArgs, CheckFormat, Cli, Command, OutputFormat, OutputMode};
pub use lang::{LangError, path_to_lang};
pub use predicate::{MatchSet, Predicate};

pub(crate) fn render_uri(
	m: &crate::core::moniker::Moniker,
	cfg: &crate::core::uri::UriConfig<'_>,
) -> String {
	crate::core::uri::to_uri(m, cfg)
		.unwrap_or_else(|_| format!("<non-utf8:{}b>", m.as_bytes().len()))
}

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

pub fn run<W1: Write, W2: Write>(cli: &Cli, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match &cli.command {
		Some(Command::Check(args)) => run_check(args, stdout, stderr),
		None => run_extract(&cli.extract, stdout, stderr),
	}
}

fn run_extract<W1: Write, W2: Write>(args: &Args, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match extract_inner(args, stdout) {
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

fn extract_inner<W: Write>(args: &Args, stdout: &mut W) -> anyhow::Result<bool> {
	let file = args
		.file
		.as_deref()
		.ok_or_else(|| anyhow::anyhow!("missing FILE argument; run `pg-moniker --help`"))?;
	let path: &Path = file;
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

fn run_check<W1: Write, W2: Write>(args: &CheckArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match check_inner(args, stdout) {
		Ok(any_violation) => {
			if any_violation {
				Exit::NoMatch
			} else {
				Exit::Match
			}
		}
		Err(e) => {
			let _ = writeln!(stderr, "pg-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn check_inner<W: Write>(args: &CheckArgs, stdout: &mut W) -> anyhow::Result<bool> {
	let path: &Path = &args.file;
	let lang = path_to_lang(path)?;
	let source = std::fs::read_to_string(path)
		.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
	let cfg = check::load_with_overrides(Some(&args.rules))?;
	let scheme = format!("{}+moniker://", lang.tag());
	let graph = extract::extract(lang, &source);
	let raw = check::evaluate(&graph, &source, lang, &cfg, &scheme)?;
	let violations = check::apply_suppressions(&graph, &source, raw);
	let any = !violations.is_empty();
	match args.format {
		CheckFormat::Text => write_violations_text(stdout, path, &violations)?,
		CheckFormat::Json => write_violations_json(stdout, path, &violations)?,
	}
	Ok(any)
}

fn write_violations_text<W: Write>(
	w: &mut W,
	path: &Path,
	violations: &[check::Violation],
) -> std::io::Result<()> {
	for v in violations {
		writeln!(
			w,
			"{}:L{}-L{} [{}] {}",
			path.display(),
			v.lines.0,
			v.lines.1,
			v.rule_id,
			v.message
		)?;
		if let Some(explanation) = &v.explanation {
			for line in explanation.trim().lines() {
				writeln!(w, "  → {line}")?;
			}
		}
	}
	Ok(())
}

fn write_violations_json<W: Write>(
	w: &mut W,
	path: &Path,
	violations: &[check::Violation],
) -> anyhow::Result<()> {
	#[derive(serde::Serialize)]
	struct V<'a> {
		rule_id: &'a str,
		moniker: &'a str,
		kind: &'a str,
		lines: [u32; 2],
		message: &'a str,
		#[serde(skip_serializing_if = "Option::is_none")]
		explanation: Option<&'a str>,
	}
	#[derive(serde::Serialize)]
	struct Out<'a> {
		file: String,
		violations: Vec<V<'a>>,
	}
	let out = Out {
		file: path.display().to_string(),
		violations: violations
			.iter()
			.map(|v| V {
				rule_id: &v.rule_id,
				moniker: &v.moniker,
				kind: &v.kind,
				lines: [v.lines.0, v.lines.1],
				message: &v.message,
				explanation: v.explanation.as_deref(),
			})
			.collect(),
	};
	serde_json::to_writer_pretty(&mut *w, &out)?;
	w.write_all(b"\n")?;
	Ok(())
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
