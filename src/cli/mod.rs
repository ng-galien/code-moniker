//! Standalone CLI surface. See `docs/cli-extract.md` (per-file probe)
//! and `docs/cli-check.md` (project linter).

pub mod args;
pub mod check;
pub mod extract;
pub mod format;
pub mod lang;
pub mod lines;
pub mod predicate;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub use args::{Args, CheckArgs, CheckFormat, Cli, Command, OutputFormat, OutputMode};
pub use lang::{LangError, path_to_lang};
pub use predicate::{MatchSet, Predicate};

pub(crate) const DEFAULT_SCHEME: &str = "code+moniker://";

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
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn extract_inner<W: Write>(args: &Args, stdout: &mut W) -> anyhow::Result<bool> {
	let file = args
		.file
		.as_deref()
		.ok_or_else(|| anyhow::anyhow!("missing FILE argument; run `code-moniker --help`"))?;
	let path: &Path = file;
	let lang = path_to_lang(path)?;
	let source = std::fs::read_to_string(path)
		.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
	let scheme = args.scheme.as_deref().unwrap_or(DEFAULT_SCHEME).to_string();
	let predicates = args.compiled_predicates(&scheme)?;
	let graph = extract::extract(lang, &source, path);
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
	match check_inner(args, stdout, stderr) {
		Ok(any_violation_or_error) => {
			if any_violation_or_error {
				Exit::NoMatch
			} else {
				Exit::Match
			}
		}
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn check_inner<W: Write, E: Write>(
	args: &CheckArgs,
	stdout: &mut W,
	stderr: &mut E,
) -> anyhow::Result<bool> {
	let path: &Path = &args.file;
	let cfg = check::load_with_overrides(Some(&args.rules))?;
	let meta = std::fs::metadata(path)
		.map_err(|e| anyhow::anyhow!("cannot stat {}: {e}", path.display()))?;
	let (reports, errors) = if meta.is_dir() {
		check_project(path, &cfg)?
	} else {
		match check_one_file(path, &cfg)? {
			Some(report) => (vec![report], Vec::new()),
			None => {
				return Err(anyhow::anyhow!(
					"{} has no recognised extension",
					path.display()
				));
			}
		}
	};
	for e in &errors {
		let _ = writeln!(
			stderr,
			"code-moniker: error reading {}: {}",
			e.path.display(),
			e.error
		);
	}
	let any_violation = reports.iter().any(|r| !r.violations.is_empty());
	match args.format {
		CheckFormat::Text => write_reports_text(stdout, &reports, &errors)?,
		CheckFormat::Json => write_reports_json(stdout, &reports, &errors)?,
	}
	Ok(any_violation || !errors.is_empty())
}

struct FileReport {
	path: PathBuf,
	violations: Vec<check::Violation>,
}

struct FileError {
	path: PathBuf,
	error: String,
}

fn check_one_file(path: &Path, cfg: &check::Config) -> anyhow::Result<Option<FileReport>> {
	let Ok(lang) = path_to_lang(path) else {
		return Ok(None);
	};
	let compiled = check::compile_rules(cfg, lang, DEFAULT_SCHEME)?;
	check_one_compiled(path, None, lang, &compiled).map(Some)
}

/// `moniker_anchor` overrides the path passed to the extractor — used by
/// project mode to anchor each file's moniker on its path relative to the
/// scan root. `None` means "same as `fs_path`" (single-file mode).
fn check_one_compiled(
	fs_path: &Path,
	moniker_anchor: Option<&Path>,
	lang: crate::lang::Lang,
	compiled: &check::CompiledRules,
) -> anyhow::Result<FileReport> {
	let source = std::fs::read_to_string(fs_path)
		.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", fs_path.display()))?;
	let graph = extract::extract(lang, &source, moniker_anchor.unwrap_or(fs_path));
	let raw = check::evaluate_compiled(&graph, &source, lang, DEFAULT_SCHEME, compiled);
	let violations = check::apply_suppressions(&graph, &source, raw);
	Ok(FileReport {
		path: fs_path.to_path_buf(),
		violations,
	})
}

/// Project-mode scan. Per-file I/O errors are accumulated in `Vec<FileError>`
/// rather than aborting the scan. Rules are compiled once per language and
/// shared across the parallel pool.
fn check_project(
	root: &Path,
	cfg: &check::Config,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	use rayon::prelude::*;
	use std::collections::HashMap;
	let paths: Vec<(PathBuf, crate::lang::Lang)> = ignore::WalkBuilder::new(root)
		.build()
		.filter_map(|entry| entry.ok())
		.filter(|e| e.file_type().is_some_and(|t| t.is_file()))
		.filter_map(|e| {
			let p = e.into_path();
			let lang = path_to_lang(&p).ok()?;
			Some((p, lang))
		})
		.collect();
	let mut compiled: HashMap<crate::lang::Lang, check::CompiledRules> = HashMap::new();
	for (_, lang) in &paths {
		if compiled.contains_key(lang) {
			continue;
		}
		compiled.insert(*lang, check::compile_rules(cfg, *lang, DEFAULT_SCHEME)?);
	}
	let outcomes: Vec<Result<FileReport, FileError>> = paths
		.par_iter()
		.map(|(p, lang)| {
			let rules = &compiled[lang];
			let rel = p.strip_prefix(root).unwrap_or(p);
			check_one_compiled(p, Some(rel), *lang, rules).map_err(|e| FileError {
				path: p.clone(),
				error: format!("{e:#}"),
			})
		})
		.collect();
	let mut reports = Vec::new();
	let mut errors = Vec::new();
	for o in outcomes {
		match o {
			Ok(r) => reports.push(r),
			Err(e) => errors.push(e),
		}
	}
	reports.sort_by(|a, b| a.path.cmp(&b.path));
	errors.sort_by(|a, b| a.path.cmp(&b.path));
	Ok((reports, errors))
}

fn write_reports_text<W: Write>(
	w: &mut W,
	reports: &[FileReport],
	errors: &[FileError],
) -> std::io::Result<()> {
	let mut total = 0usize;
	let mut files_with = 0usize;
	for r in reports {
		if r.violations.is_empty() {
			continue;
		}
		files_with += 1;
		total += r.violations.len();
		for v in &r.violations {
			writeln!(
				w,
				"{}:L{}-L{} [{}] {}",
				r.path.display(),
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
	}
	// Suppress the footer only in the per-edit happy path: one file scanned,
	// clean, no errors. That preserves the existing single-file hook UX
	// while every multi-file or noisy run gets a one-line summary.
	let single_clean = reports.len() == 1 && files_with == 0 && errors.is_empty();
	if !single_clean {
		write!(
			w,
			"\n{total} violation(s) across {files_with} file(s) ({} scanned",
			reports.len()
		)?;
		if !errors.is_empty() {
			write!(w, ", {} file(s) errored", errors.len())?;
		}
		writeln!(w, ").")?;
	}
	Ok(())
}

fn write_reports_json<W: Write>(
	w: &mut W,
	reports: &[FileReport],
	errors: &[FileError],
) -> anyhow::Result<()> {
	#[derive(serde::Serialize)]
	struct FileEntry<'a> {
		file: String,
		violations: &'a [check::Violation],
	}
	#[derive(serde::Serialize)]
	struct ErrorEntry<'a> {
		file: String,
		error: &'a str,
	}
	#[derive(serde::Serialize)]
	struct Summary {
		files_scanned: usize,
		files_with_violations: usize,
		total_violations: usize,
		files_with_errors: usize,
	}
	#[derive(serde::Serialize)]
	struct Out<'a> {
		summary: Summary,
		files: Vec<FileEntry<'a>>,
		#[serde(skip_serializing_if = "Vec::is_empty")]
		errors: Vec<ErrorEntry<'a>>,
	}
	let files: Vec<FileEntry> = reports
		.iter()
		.map(|r| FileEntry {
			file: r.path.display().to_string(),
			violations: &r.violations,
		})
		.collect();
	let total = files.iter().map(|f| f.violations.len()).sum();
	let files_with = files.iter().filter(|f| !f.violations.is_empty()).count();
	let err_entries: Vec<ErrorEntry> = errors
		.iter()
		.map(|e| ErrorEntry {
			file: e.path.display().to_string(),
			error: &e.error,
		})
		.collect();
	let out = Out {
		summary: Summary {
			files_scanned: files.len(),
			files_with_violations: files_with,
			total_violations: total,
			files_with_errors: err_entries.len(),
		},
		files,
		errors: err_entries,
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
