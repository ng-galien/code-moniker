//! CLI adapter for the `code-moniker-check` engine. Owns argument mapping,
//! output rendering (text / JSON / codex-hook), and the process exit verdict.
//! The engine produces structured [`check::FileReport`]/[`check::FileError`]
//! values; everything terminal-facing lives here.

use std::io::Write;
use std::path::Path;

use check::{
	CheckRequest, CheckRun, CheckSkipReason, DefaultRulesSelection, FileError, FileReport,
	RuleSetRequest,
};
use code_moniker_check as check;

use crate::args::{CheckArgs, CheckFormat, DefaultRules};
use crate::{DEFAULT_SCHEME, Exit};

pub fn run<W1: Write, W2: Write>(args: &CheckArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	if let Some(scenario) = &args.scenario {
		if args.format == CheckFormat::Json {
			return run_scenario_check(args, scenario, stdout, stderr);
		}
		return crate::check_scenario::run(scenario, stdout, stderr);
	}
	let request = check_request_from_args(args);
	match run_request_inner(
		request,
		args.format,
		args.report,
		args.max_violations,
		stdout,
		stderr,
	) {
		Ok(outcome) => {
			if outcome.any_error
				|| (outcome.any_error_violation && args.format != CheckFormat::CodexHook)
			{
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

fn run_scenario_check<W1: Write, W2: Write>(
	args: &CheckArgs,
	scenario: &Path,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	match crate::check_scenario::check_run(scenario, &args.files, args.report) {
		Ok(run) => {
			if should_render(args.format, &run)
				&& let Err(error) = write_reports_json(stdout, &run, args.report)
			{
				let _ = writeln!(stderr, "code-moniker: {error:#}");
				return Exit::UsageError;
			}
			let outcome = CheckOutcome::from_run(&run);
			if outcome.any_error || outcome.any_error_violation {
				Exit::NoMatch
			} else {
				Exit::Match
			}
		}
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

#[cfg(feature = "mcp")]
pub(crate) fn run_text_request<W1: Write, W2: Write>(
	request: CheckRequest,
	report: bool,
	max_violations: Option<usize>,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	match run_request_inner(
		request,
		CheckFormat::Text,
		report,
		max_violations,
		stdout,
		stderr,
	) {
		Ok(outcome) => {
			if outcome.any_error || outcome.any_error_violation {
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

fn run_request_inner<W: Write, E: Write>(
	request: CheckRequest,
	format: CheckFormat,
	report: bool,
	max_violations: Option<usize>,
	stdout: &mut W,
	stderr: &mut E,
) -> anyhow::Result<CheckOutcome> {
	let run = request.run()?;
	if !should_render(format, &run) {
		return Ok(CheckOutcome::from_run(&run));
	}
	for e in &run.errors {
		let _ = writeln!(
			stderr,
			"code-moniker: error reading {}: {}",
			e.path.display(),
			e.error
		);
	}
	match format {
		CheckFormat::Text => write_reports_text(stdout, &run, report, max_violations)?,
		CheckFormat::Json => write_reports_json(stdout, &run, report)?,
		CheckFormat::CodexHook => write_reports_codex_hook(stdout, &run, max_violations)?,
	}
	Ok(CheckOutcome::from_run(&run))
}

fn check_request_from_args(args: &CheckArgs) -> CheckRequest {
	CheckRequest::new(args.path.clone(), ruleset_request_from_args(args))
		.with_report(args.report)
		.with_files(args.files.clone())
}

fn ruleset_request_from_args(args: &CheckArgs) -> RuleSetRequest {
	RuleSetRequest::with_rules(args.rules.clone(), DEFAULT_SCHEME)
		.with_inline_rules(args.rules_inline.clone())
		.with_default_rules(DefaultRulesSelection::from_override(
			args.default_rules.map(DefaultRules::enabled),
		))
		.with_profile(args.profile.clone())
}

fn should_render(format: CheckFormat, run: &CheckRun) -> bool {
	match run.skip_reason {
		None => true,
		Some(CheckSkipReason::ExcludedSingleFile | CheckSkipReason::NoMatchingFiles) => {
			format == CheckFormat::Json
		}
		Some(CheckSkipReason::UnsupportedSingleFile) => false,
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CheckOutcome {
	any_error_violation: bool,
	any_error: bool,
}

impl CheckOutcome {
	fn from_run(run: &CheckRun) -> Self {
		Self {
			any_error_violation: run.any_error_violation(),
			any_error: run.any_error(),
		}
	}
}

struct ViolationEntry<'a> {
	path: &'a Path,
	violation: &'a check::Violation,
}

/// Single-file clean runs (one report, zero violations, zero errors) skip the
/// trailing summary so per-edit PostToolUse hooks stay silent. Every other
/// shape emits the `N violation(s) across M file(s) (K scanned)` footer.
fn write_reports_text<W: Write>(
	w: &mut W,
	run: &CheckRun,
	include_rule_report: bool,
	max_violations: Option<usize>,
) -> std::io::Result<()> {
	let reports = &run.reports;
	let errors = &run.errors;
	let counts = run.violation_counts();
	let selected = max_violations.map(|max| largest_violation_group(reports, max));
	if let Some(selected) = &selected {
		if let Some(first) = selected.first() {
			let group_label = if first.violation.severity.is_error() && counts.warnings > 0 {
				"error rule group"
			} else {
				"rule group"
			};
			writeln!(
				w,
				"Showing {selected_len} of {total} violation(s) from largest {group_label} `{rule_id}`.",
				selected_len = selected.len(),
				total = counts.total,
				group_label = group_label,
				rule_id = first.violation.rule_id
			)?;
		}
		for entry in selected {
			write_violation_text(w, entry.path, entry.violation)?;
		}
	} else {
		for r in reports {
			for v in &r.violations {
				write_violation_text(w, &r.path, v)?;
			}
		}
	}
	let single_clean = reports.len() == 1 && counts.files_with == 0 && errors.is_empty();
	if !single_clean {
		write!(
			w,
			"\n{total} violation(s) across {files_with} file(s) ({scanned} scanned, elapsed {elapsed_ms} ms",
			total = counts.total,
			files_with = counts.files_with,
			scanned = reports.len(),
			elapsed_ms = run.elapsed_ms
		)?;
		if counts.warnings > 0 {
			write!(
				w,
				", {} error violation(s), {} warning(s)",
				counts.errors, counts.warnings
			)?;
		}
		if !errors.is_empty() {
			write!(w, ", {} file(s) errored", errors.len())?;
		}
		writeln!(w, ").")?;
		write_failed_rules_text(w, run)?;
		if !errors.is_empty() {
			writeln!(w, "Read errors: {} file(s).", errors.len())?;
		}
	}
	if include_rule_report {
		write_rule_report_text(w, run)?;
	}
	Ok(())
}

fn write_violation_text<W: Write>(
	w: &mut W,
	path: &Path,
	v: &check::Violation,
) -> std::io::Result<()> {
	let severity_prefix = if v.severity.is_warn() {
		"warning: "
	} else {
		""
	};
	writeln!(
		w,
		"{}:L{}-L{} [{}] {}{}",
		path.display(),
		v.lines.0,
		v.lines.1,
		v.rule_id,
		severity_prefix,
		v.message
	)?;
	if let Some(explanation) = &v.explanation {
		for line in explanation.trim().lines() {
			writeln!(w, "  -> {line}")?;
		}
	}
	Ok(())
}

fn write_failed_rules_text<W: Write>(w: &mut W, run: &CheckRun) -> std::io::Result<()> {
	let failed_rules = run.failed_rule_summary();
	if failed_rules.is_empty() {
		return Ok(());
	}
	writeln!(w, "Failed rules:")?;
	for item in failed_rules {
		if item.severity.is_warn() {
			writeln!(w, "- {}: {} warning(s)", item.rule_id, item.violations)?;
		} else {
			writeln!(w, "- {}: {} violation(s)", item.rule_id, item.violations)?;
		}
	}
	Ok(())
}

fn write_rule_report_text<W: Write>(w: &mut W, run: &CheckRun) -> std::io::Result<()> {
	let rule_reports = aggregate_rule_reports(&run.reports);
	if rule_reports.is_empty() {
		return Ok(());
	}
	writeln!(w, "\nRule report:")?;
	for r in rule_reports {
		write!(
			w,
			"- {}: domain={}, evaluated={}, matches={}, violations={}",
			r.rule_id, r.domain, r.evaluated, r.matches, r.violations
		)?;
		if r.severity.is_warn() {
			write!(w, ", severity=warn")?;
		}
		if let Some(n) = r.antecedent_matches {
			write!(w, ", antecedent_matches={n}")?;
		}
		if let Some(warning) = r.warning {
			write!(w, " warning: {warning}")?;
		}
		writeln!(w)?;
	}
	Ok(())
}

fn write_reports_json<W: Write>(
	w: &mut W,
	run: &CheckRun,
	include_rule_report: bool,
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
	struct Out<'a> {
		summary: check::CheckSummary,
		files: Vec<FileEntry<'a>>,
		#[serde(skip_serializing_if = "Vec::is_empty")]
		errors: Vec<ErrorEntry<'a>>,
		#[serde(skip_serializing_if = "Vec::is_empty")]
		rule_report: Vec<check::RuleReport>,
	}
	let reports = &run.reports;
	let errors = &run.errors;
	let files: Vec<FileEntry> = reports
		.iter()
		.map(|r| FileEntry {
			file: r.path.display().to_string(),
			violations: &r.violations,
		})
		.collect();
	let err_entries: Vec<ErrorEntry> = errors
		.iter()
		.map(|e| ErrorEntry {
			file: e.path.display().to_string(),
			error: &e.error,
		})
		.collect();
	let out = Out {
		summary: run.summary(),
		files,
		errors: err_entries,
		rule_report: if include_rule_report {
			aggregate_rule_reports(reports)
		} else {
			Vec::new()
		},
	};
	serde_json::to_writer_pretty(&mut *w, &out)?;
	w.write_all(b"\n")?;
	Ok(())
}

fn write_reports_codex_hook<W: Write>(
	w: &mut W,
	run: &CheckRun,
	max_violations: Option<usize>,
) -> anyhow::Result<()> {
	let error_reports = reports_with_severity(&run.reports, check::RuleSeverity::Error);
	let any_error_violation = error_reports
		.iter()
		.any(|report| !report.violations.is_empty());
	if !any_error_violation {
		return Ok(());
	}
	let reason = codex_hook_reason(&error_reports, &run.errors, run.elapsed_ms, max_violations)?;
	serde_json::to_writer(
		&mut *w,
		&serde_json::json!({
			"decision": "block",
			"reason": reason,
		}),
	)?;
	w.write_all(b"\n")?;
	Ok(())
}

fn codex_hook_reason(
	reports: &[FileReport],
	errors: &[FileError],
	elapsed_ms: u64,
	max_violations: Option<usize>,
) -> anyhow::Result<String> {
	let mut reason = Vec::new();
	writeln!(
		&mut reason,
		"code-moniker architecture check failed. Fix the reported rule violation(s):"
	)?;
	let run = CheckRun {
		reports: reports.to_vec(),
		errors: errors.to_vec(),
		elapsed_ms,
		skip_reason: None,
	};
	write_reports_text(&mut reason, &run, false, max_violations)?;
	Ok(String::from_utf8(reason)?)
}

fn largest_violation_group<'a>(reports: &'a [FileReport], max: usize) -> Vec<ViolationEntry<'a>> {
	use std::collections::BTreeMap;
	let mut by_rule: BTreeMap<&str, Vec<ViolationEntry<'a>>> = BTreeMap::new();
	let prefer_errors = reports.iter().any(|report| {
		report
			.violations
			.iter()
			.any(|violation| violation.severity.is_error())
	});
	for report in reports {
		for violation in &report.violations {
			if prefer_errors && !violation.severity.is_error() {
				continue;
			}
			by_rule
				.entry(violation.rule_id.as_str())
				.or_default()
				.push(ViolationEntry {
					path: &report.path,
					violation,
				});
		}
	}
	let Some((_, mut group)) =
		by_rule
			.into_iter()
			.max_by(|(left_rule, left), (right_rule, right)| {
				left.len()
					.cmp(&right.len())
					.then_with(|| right_rule.cmp(left_rule))
			})
	else {
		return Vec::new();
	};
	group.sort_by(|a, b| {
		a.path
			.cmp(b.path)
			.then_with(|| a.violation.lines.cmp(&b.violation.lines))
			.then_with(|| a.violation.message.cmp(&b.violation.message))
	});
	group.truncate(max);
	group
}

fn aggregate_rule_reports(reports: &[FileReport]) -> Vec<check::RuleReport> {
	use std::collections::BTreeMap;
	let mut by_rule: BTreeMap<String, check::RuleReport> = BTreeMap::new();
	for report in reports {
		for item in &report.rule_reports {
			by_rule
				.entry(item.rule_id.clone())
				.and_modify(|acc| {
					acc.evaluated += item.evaluated;
					acc.matches += item.matches;
					acc.violations += item.violations;
					if let Some(n) = item.antecedent_matches {
						acc.antecedent_matches = Some(acc.antecedent_matches.unwrap_or(0) + n);
					}
				})
				.or_insert_with(|| item.clone());
		}
	}
	let mut out: Vec<_> = by_rule.into_values().collect();
	for report in &mut out {
		if report.evaluated > 0 && report.antecedent_matches == Some(0) {
			report.warning = Some("antecedent never matched".to_string());
		} else {
			report.warning = None;
		}
	}
	out
}

fn reports_with_severity(reports: &[FileReport], severity: check::RuleSeverity) -> Vec<FileReport> {
	reports
		.iter()
		.map(|report| FileReport {
			path: report.path.clone(),
			violations: report
				.violations
				.iter()
				.filter(|violation| violation.severity == severity)
				.cloned()
				.collect(),
			rule_reports: Vec::new(),
		})
		.collect()
}
