use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use code_moniker_workspace::environment;

use crate::args::{CheckArgs, CheckFormat, DefaultRules};
use crate::{DEFAULT_SCHEME, Exit, check, path_to_lang};

pub fn run<W1: Write, W2: Write>(args: &CheckArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match check_inner(args, stdout, stderr) {
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

fn check_inner<W: Write, E: Write>(
	args: &CheckArgs,
	stdout: &mut W,
	stderr: &mut E,
) -> anyhow::Result<CheckOutcome> {
	let started = Instant::now();
	let path: &Path = &args.path;
	let mut cfg = check::load_with_cli_default_rules(
		Some(&args.rules),
		args.default_rules.map(DefaultRules::enabled),
	)?;
	if let Some(name) = &args.profile {
		cfg.apply_profile(name)?;
	}
	let meta = std::fs::metadata(path)
		.map_err(|e| anyhow::anyhow!("cannot stat {}: {e}", path.display()))?;
	let (reports, errors) = if meta.is_dir() {
		if args.files.is_empty() {
			check_project(path, &cfg, args.report)?
		} else {
			check_project_files(path, &args.files, &cfg, args.report)?
		}
	} else {
		if !args.files.is_empty() {
			anyhow::bail!("--file can only be used when check PATH is a directory");
		}
		let excluded = check::UriExclusionMatcher::new(&cfg.exclude.uris).matches_path(path);
		match check_one_file(path, &cfg, args.report)? {
			Some(report) => (vec![report], Vec::new()),
			None if excluded && args.format == CheckFormat::Json => (Vec::new(), Vec::new()),
			None => {
				return Ok(CheckOutcome {
					any_error_violation: false,
					any_error: false,
				});
			}
		}
	};
	if !args.files.is_empty()
		&& reports.is_empty()
		&& errors.is_empty()
		&& args.format != CheckFormat::Json
	{
		return Ok(CheckOutcome {
			any_error_violation: false,
			any_error: false,
		});
	}
	for e in &errors {
		let _ = writeln!(
			stderr,
			"code-moniker: error reading {}: {}",
			e.path.display(),
			e.error
		);
	}
	let any_error_violation = reports.iter().any(|r| {
		r.violations
			.iter()
			.any(|violation| violation.severity.is_error())
	});
	let elapsed = started.elapsed();
	match args.format {
		CheckFormat::Text => write_reports_text(
			stdout,
			&reports,
			&errors,
			args.report,
			elapsed,
			args.max_violations,
		)?,
		CheckFormat::Json => write_reports_json(stdout, &reports, &errors, args.report, elapsed)?,
		CheckFormat::CodexHook => {
			write_reports_codex_hook(stdout, &reports, &errors, elapsed, args.max_violations)?
		}
	}
	Ok(CheckOutcome {
		any_error_violation,
		any_error: !errors.is_empty(),
	})
}

struct FileReport {
	path: PathBuf,
	violations: Vec<check::Violation>,
	rule_reports: Vec<check::RuleReport>,
}

struct FileError {
	path: PathBuf,
	error: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CheckOutcome {
	any_error_violation: bool,
	any_error: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
struct FailedRuleSummary {
	rule_id: String,
	severity: check::RuleSeverity,
	violations: usize,
}

struct ViolationEntry<'a> {
	path: &'a Path,
	violation: &'a check::Violation,
}

#[derive(Default)]
struct ViolationCounts {
	total: usize,
	errors: usize,
	warnings: usize,
	files_with: usize,
}

fn check_one_file(
	path: &Path,
	cfg: &check::Config,
	report: bool,
) -> anyhow::Result<Option<FileReport>> {
	let Ok(lang) = path_to_lang(path) else {
		return Ok(None);
	};
	let excludes = check::UriExclusionMatcher::new(&cfg.exclude.uris);
	if excludes.matches_path(path) {
		return Ok(None);
	}
	let compiled = check::compile_rules(cfg, lang, DEFAULT_SCHEME)?;
	check_one_compiled(path, None, lang, &compiled, report).map(Some)
}

/// `moniker_anchor` overrides the path passed to the extractor - used by
/// project mode to anchor each file's moniker on its path relative to the
/// scan root. `None` means "same as `fs_path`" (single-file mode).
fn check_one_compiled(
	fs_path: &Path,
	moniker_anchor: Option<&Path>,
	lang: code_moniker_core::lang::Lang,
	compiled: &check::CompiledRules,
	report: bool,
) -> anyhow::Result<FileReport> {
	let source = std::fs::read_to_string(fs_path)
		.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", fs_path.display()))?;
	let graph = environment::extract_source_with(
		lang,
		&source,
		moniker_anchor.unwrap_or(fs_path),
		&environment::ExtractContext::default(),
	);
	let raw = check::evaluate_compiled(&graph, &source, lang, DEFAULT_SCHEME, compiled);
	let violations = check::apply_suppressions(&graph, &source, raw);
	let rule_reports = if report {
		let mut rule_reports =
			check::rule_report_compiled(&graph, &source, lang, DEFAULT_SCHEME, compiled);
		align_report_violations_with_suppressions(&mut rule_reports, &violations);
		rule_reports
	} else {
		Vec::new()
	};
	Ok(FileReport {
		path: fs_path.to_path_buf(),
		violations,
		rule_reports,
	})
}

fn check_source_file_compiled(
	file: &environment::SourceFile,
	ctx: &environment::ExtractContext,
	compiled: &check::CompiledRules,
	report: bool,
) -> anyhow::Result<FileReport> {
	let source = std::fs::read_to_string(&file.path)
		.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", file.path.display()))?;
	let graph = environment::extract_source_with(file.lang, &source, &file.anchor, ctx);
	let raw = check::evaluate_compiled(&graph, &source, file.lang, DEFAULT_SCHEME, compiled);
	let violations = check::apply_suppressions(&graph, &source, raw);
	let rule_reports = if report {
		let mut rule_reports =
			check::rule_report_compiled(&graph, &source, file.lang, DEFAULT_SCHEME, compiled);
		align_report_violations_with_suppressions(&mut rule_reports, &violations);
		rule_reports
	} else {
		Vec::new()
	};
	Ok(FileReport {
		path: file.path.clone(),
		violations,
		rule_reports,
	})
}

/// Project-mode scan. Per-file I/O errors are accumulated in `Vec<FileError>`
/// rather than aborting the scan. Rules are compiled once per language and
/// shared across the parallel pool.
fn check_project(
	root: &Path,
	cfg: &check::Config,
	report: bool,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	let source_set = environment::discover_sources(&[root.to_path_buf()], None)?;
	check_source_set(&source_set, cfg, report)
}

fn check_project_files(
	root: &Path,
	files: &[PathBuf],
	cfg: &check::Config,
	report: bool,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	let source_set = environment::discover_source_files(root, files, None)?;
	check_source_set(&source_set, cfg, report)
}

fn check_source_set(
	source_set: &environment::SourceFileSet,
	cfg: &check::Config,
	report: bool,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	use rayon::prelude::*;
	use std::collections::HashMap;
	let excludes = check::UriExclusionMatcher::new(&cfg.exclude.uris);
	let mut compiled: HashMap<code_moniker_core::lang::Lang, check::CompiledRules> = HashMap::new();
	let files: Vec<&environment::SourceFile> = source_set
		.files
		.iter()
		.filter(|f| !excludes.matches_path(&f.path))
		.collect();
	for f in &files {
		if compiled.contains_key(&f.lang) {
			continue;
		}
		compiled.insert(f.lang, check::compile_rules(cfg, f.lang, DEFAULT_SCHEME)?);
	}
	let outcomes: Vec<Result<FileReport, FileError>> = files
		.par_iter()
		.map(|f| {
			let f = *f;
			let rules = &compiled[&f.lang];
			let ctx = &source_set.roots[f.source].ctx;
			check_source_file_compiled(f, ctx, rules, report).map_err(|e| FileError {
				path: f.path.clone(),
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

fn align_report_violations_with_suppressions(
	rule_reports: &mut [check::RuleReport],
	violations: &[check::Violation],
) {
	use std::collections::HashMap;
	let mut counts: HashMap<&str, usize> = HashMap::new();
	for v in violations {
		*counts.entry(v.rule_id.as_str()).or_insert(0) += 1;
	}
	for report in rule_reports {
		report.violations = counts.get(report.rule_id.as_str()).copied().unwrap_or(0);
	}
}

/// Single-file clean runs (one report, zero violations, zero errors) skip the
/// trailing summary so per-edit PostToolUse hooks stay silent. Every other
/// shape emits the `N violation(s) across M file(s) (K scanned)` footer.
fn write_reports_text<W: Write>(
	w: &mut W,
	reports: &[FileReport],
	errors: &[FileError],
	include_rule_report: bool,
	elapsed: Duration,
	max_violations: Option<usize>,
) -> std::io::Result<()> {
	let counts = violation_counts(reports);
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
			elapsed_ms = duration_ms(elapsed)
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
		write_failed_rules_text(w, reports)?;
		if !errors.is_empty() {
			writeln!(w, "Read errors: {} file(s).", errors.len())?;
		}
	}
	if include_rule_report {
		write_rule_report_text(w, reports)?;
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

fn violation_counts(reports: &[FileReport]) -> ViolationCounts {
	let mut counts = ViolationCounts::default();
	for report in reports {
		if report.violations.is_empty() {
			continue;
		}
		counts.files_with += 1;
		for violation in &report.violations {
			counts.total += 1;
			if violation.severity.is_error() {
				counts.errors += 1;
			} else {
				counts.warnings += 1;
			}
		}
	}
	counts
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

fn write_failed_rules_text<W: Write>(w: &mut W, reports: &[FileReport]) -> std::io::Result<()> {
	let failed_rules = failed_rule_summary(reports);
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

fn failed_rule_summary(reports: &[FileReport]) -> Vec<FailedRuleSummary> {
	use std::collections::BTreeMap;
	let mut by_rule: BTreeMap<(String, check::RuleSeverity), usize> = BTreeMap::new();
	for report in reports {
		for violation in &report.violations {
			*by_rule
				.entry((violation.rule_id.clone(), violation.severity))
				.or_default() += 1;
		}
	}
	let mut out: Vec<_> = by_rule
		.into_iter()
		.map(|((rule_id, severity), violations)| FailedRuleSummary {
			rule_id,
			severity,
			violations,
		})
		.collect();
	out.sort_by(|a, b| {
		b.violations
			.cmp(&a.violations)
			.then_with(|| b.severity.cmp(&a.severity))
			.then_with(|| a.rule_id.cmp(&b.rule_id))
	});
	out
}

fn write_rule_report_text<W: Write>(w: &mut W, reports: &[FileReport]) -> std::io::Result<()> {
	let rule_reports = aggregate_rule_reports(reports);
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
	for r in &mut out {
		if r.evaluated > 0 && r.antecedent_matches == Some(0) {
			r.warning = Some("antecedent never matched".to_string());
		} else {
			r.warning = None;
		}
	}
	out
}

fn write_reports_json<W: Write>(
	w: &mut W,
	reports: &[FileReport],
	errors: &[FileError],
	include_rule_report: bool,
	elapsed: Duration,
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
		total_rule_errors: usize,
		total_warnings: usize,
		files_with_errors: usize,
		total_errors: usize,
		elapsed_ms: u64,
		failed_rules: Vec<FailedRuleSummary>,
	}
	#[derive(serde::Serialize)]
	struct Out<'a> {
		summary: Summary,
		files: Vec<FileEntry<'a>>,
		#[serde(skip_serializing_if = "Vec::is_empty")]
		errors: Vec<ErrorEntry<'a>>,
		#[serde(skip_serializing_if = "Vec::is_empty")]
		rule_report: Vec<check::RuleReport>,
	}
	let files: Vec<FileEntry> = reports
		.iter()
		.map(|r| FileEntry {
			file: r.path.display().to_string(),
			violations: &r.violations,
		})
		.collect();
	let counts = violation_counts(reports);
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
			files_with_violations: counts.files_with,
			total_violations: counts.total,
			total_rule_errors: counts.errors,
			total_warnings: counts.warnings,
			files_with_errors: err_entries.len(),
			total_errors: err_entries.len(),
			elapsed_ms: duration_ms(elapsed),
			failed_rules: failed_rule_summary(reports),
		},
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
	reports: &[FileReport],
	errors: &[FileError],
	elapsed: Duration,
	max_violations: Option<usize>,
) -> anyhow::Result<()> {
	let error_reports = reports_with_severity(reports, check::RuleSeverity::Error);
	let any_error_violation = error_reports
		.iter()
		.any(|report| !report.violations.is_empty());
	if !any_error_violation {
		return Ok(());
	}
	let reason = codex_hook_reason(&error_reports, errors, elapsed, max_violations)?;
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
	elapsed: Duration,
	max_violations: Option<usize>,
) -> anyhow::Result<String> {
	let mut reason = Vec::new();
	writeln!(
		&mut reason,
		"code-moniker architecture check failed. Fix the reported rule violation(s):"
	)?;
	write_reports_text(&mut reason, reports, errors, false, elapsed, max_violations)?;
	Ok(String::from_utf8(reason)?)
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

fn duration_ms(duration: Duration) -> u64 {
	duration.as_millis().try_into().unwrap_or(u64::MAX)
}
