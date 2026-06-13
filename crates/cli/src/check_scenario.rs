//! CLI adapter for executable check scenarios (unstable surface). Reads a
//! scenario document into the in-memory check workspace and either verifies or
//! blesses its expectations.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use code_moniker_check::CheckRun;
use code_moniker_check::scenario::{Scenario, ScenarioRun};

use crate::{DEFAULT_SCHEME, Exit};

const BLESS_ENV: &str = "CM_SCENARIO_BLESS";

pub(crate) fn run<W1: Write, W2: Write>(path: &Path, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match run_inner(path, stdout) {
		Ok(true) => Exit::Match,
		Ok(false) => Exit::NoMatch,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

fn run_inner<W: Write>(path: &Path, stdout: &mut W) -> anyhow::Result<bool> {
	let document = read_document(path)?;
	let scenario = Scenario::parse(&document)?;
	let run = scenario.run(Path::new("."), DEFAULT_SCHEME)?;
	if bless_requested() {
		return bless_document(path, &document, &scenario, &run, stdout);
	}
	render_outcome(&run, stdout)
}

pub(crate) fn check_run(path: &Path, files: &[PathBuf], report: bool) -> anyhow::Result<CheckRun> {
	let document = read_document(path)?;
	let scenario = Scenario::parse(&document)?;
	let mut run = scenario.check(Path::new("."), files, DEFAULT_SCHEME, report)?;
	normalize_paths_for_catalog(&mut run);
	Ok(run)
}

fn read_document(path: &Path) -> anyhow::Result<String> {
	if path == Path::new("-") {
		let mut document = String::new();
		std::io::stdin().read_to_string(&mut document)?;
		return Ok(document);
	}
	std::fs::read_to_string(path)
		.map_err(|error| anyhow::anyhow!("cannot read scenario {}: {error}", path.display()))
}

fn bless_requested() -> bool {
	std::env::var_os(BLESS_ENV).is_some_and(|value| value == "1")
}

fn bless_document<W: Write>(
	path: &Path,
	document: &str,
	scenario: &Scenario,
	run: &ScenarioRun,
	stdout: &mut W,
) -> anyhow::Result<bool> {
	if path == Path::new("-") {
		anyhow::bail!("cannot bless a scenario read from stdin");
	}
	let blessed = scenario.bless(document, &run.actual);
	if blessed == document {
		writeln!(
			stdout,
			"scenario: expectations already match ({} violation(s))",
			run.actual.len()
		)?;
	} else {
		std::fs::write(path, blessed)?;
		writeln!(
			stdout,
			"scenario: blessed {} expectation(s)",
			run.actual.len()
		)?;
	}
	Ok(true)
}

fn render_outcome<W: Write>(run: &ScenarioRun, stdout: &mut W) -> anyhow::Result<bool> {
	for violation in &run.actual {
		writeln!(stdout, "{violation}")?;
	}
	if !run.silent_rules.is_empty() {
		writeln!(stdout, "silent rules: {}", run.silent_rules.join(", "))?;
	}
	if run.is_match() {
		writeln!(
			stdout,
			"scenario: ok ({} expected violation(s) matched)",
			run.actual.len()
		)?;
		Ok(true)
	} else {
		writeln!(stdout, "{}", run.mismatch_summary())?;
		writeln!(stdout, "scenario: mismatch")?;
		Ok(false)
	}
}

fn normalize_paths_for_catalog(run: &mut CheckRun) {
	for report in &mut run.reports {
		report.path = report
			.path
			.strip_prefix(".")
			.unwrap_or(&report.path)
			.to_path_buf();
	}
	for error in &mut run.errors {
		error.path = error
			.path
			.strip_prefix(".")
			.unwrap_or(&error.path)
			.to_path_buf();
	}
}
