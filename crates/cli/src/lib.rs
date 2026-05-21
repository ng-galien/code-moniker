//! Standalone CLI surface. See `docs/cli/extract.md` (per-file probe)
//! and `docs/cli/check.md` (project linter).

pub mod args;
pub mod cache;
pub mod check;
pub(crate) mod color;
pub mod dir;
pub mod extract;
pub mod format;
pub mod harness;
#[deprecated(note = "use code_moniker_cli::workspace::index instead")]
pub mod inspect;
pub mod lang;
pub mod lines;
pub mod manifest;
#[cfg(feature = "tui")]
pub(crate) mod perf;
pub mod predicate;
pub mod rules;
pub mod sources;
pub mod stats;
#[cfg(feature = "pretty")]
pub(crate) mod tree;
pub mod tsconfig;
#[cfg(feature = "tui")]
pub mod ui;
pub mod walk;
pub mod workspace;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

#[cfg(feature = "tui")]
pub use args::UiArgs;
pub use args::{
	CheckArgs, CheckFormat, Cli, CodexHarnessArgs, Command, DefaultRules, ExtractArgs, HarnessArgs,
	HarnessCommand, LangsArgs, LangsFormat, ManifestArgs, ManifestFormat, OutputFormat, OutputMode,
	RulesArgs, RulesCommand, RulesFileArgs, RulesLearnArgs, RulesLearnFormat, RulesShowArgs,
	RulesShowFormat, ShapesArgs, StatsArgs, StatsFormat,
};
pub use lang::{LangError, path_to_lang};
pub use predicate::{MatchSet, Predicate};

pub(crate) const DEFAULT_SCHEME: &str = "code+moniker://";

pub(crate) fn unknown_kinds_error(
	unknown: &[String],
	langs: &[code_moniker_core::lang::Lang],
	known: &std::collections::BTreeSet<&'static str>,
) -> anyhow::Error {
	let lang_tags: Vec<&str> = langs.iter().map(|l| l.tag()).collect();
	let known_list: Vec<&str> = known.iter().copied().collect();
	anyhow::anyhow!(
		"unknown --kind {} (langs in scope: {}; known kinds: {})",
		unknown.join(", "),
		lang_tags.join(", "),
		known_list.join(", "),
	)
}

pub(crate) fn render_uri(
	m: &code_moniker_core::core::moniker::Moniker,
	cfg: &code_moniker_core::core::uri::UriConfig<'_>,
) -> String {
	code_moniker_core::core::uri::to_uri(m, cfg)
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
		Command::Extract(args) => run_extract(args, stdout, stderr),
		Command::Stats(args) => stats::run(args, stdout, stderr),
		Command::Check(args) => run_check(args, stdout, stderr),
		Command::Rules(args) => rules::run(args, stdout, stderr),
		Command::Harness(args) => harness::run(args, stdout, stderr),
		#[cfg(feature = "tui")]
		Command::Ui(args) => ui::run(args, stdout, stderr),
		Command::Langs(args) => run_langs(args, stdout, stderr),
		Command::Shapes(args) => run_shapes(args, stdout, stderr),
		Command::Manifest(args) => run_manifest(args, stdout, stderr),
	}
}

fn run_manifest<W1: Write, W2: Write>(
	args: &ManifestArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	match manifest::run(args, stdout, stderr) {
		0 => Exit::Match,
		1 => Exit::NoMatch,
		_ => Exit::UsageError,
	}
}

fn shape_description(shape: code_moniker_core::core::shape::Shape) -> &'static str {
	use code_moniker_core::core::shape::Shape;
	match shape {
		Shape::Namespace => "container scopes (module, namespace, schema, impl)",
		Shape::Type => {
			"type-like declarations (class, struct, enum, interface, trait, table, view, …)"
		}
		Shape::Callable => {
			"executable code (function, method, constructor, procedure, async_function)"
		}
		Shape::Value => "named bindings (field, const, static, enum_constant, param, local, …)",
		Shape::Annotation => "attached metadata (comment) — not a structural scope",
		Shape::Ref => {
			"cross-record references (calls, imports_*, extends, uses_type, …) — marker shape for ref records"
		}
	}
}

fn run_shapes<W1: Write, W2: Write>(args: &ShapesArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match shapes_inner(args, stdout) {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn shapes_inner<W: Write>(args: &ShapesArgs, stdout: &mut W) -> anyhow::Result<()> {
	use code_moniker_core::core::shape::Shape;
	match args.format {
		LangsFormat::Text => {
			writeln!(
				stdout,
				"Each def's `kind` maps to exactly one shape; refs share `ref` as marker."
			)?;
			writeln!(
				stdout,
				"Filter with `--shape <NAME>`; `code-moniker langs <TAG>` shows the kind↔shape map per language."
			)?;
			writeln!(stdout)?;
			let width = Shape::ALL
				.iter()
				.map(|s| s.as_str().len())
				.max()
				.unwrap_or(0);
			for shape in Shape::ALL {
				writeln!(
					stdout,
					"  {:<width$}  {}",
					shape.as_str(),
					shape_description(*shape),
					width = width
				)?;
			}
		}
		LangsFormat::Json => {
			#[derive(serde::Serialize)]
			struct Entry<'a> {
				name: &'a str,
				description: &'a str,
			}
			let entries: Vec<Entry> = Shape::ALL
				.iter()
				.map(|s| Entry {
					name: s.as_str(),
					description: shape_description(*s),
				})
				.collect();
			serde_json::to_writer_pretty(&mut *stdout, &entries)?;
			stdout.write_all(b"\n")?;
		}
	}
	Ok(())
}

fn run_langs<W1: Write, W2: Write>(args: &LangsArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match langs_inner(args, stdout) {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn collect_kinds(
	lang: code_moniker_core::lang::Lang,
) -> Vec<(&'static str, code_moniker_core::core::shape::Shape)> {
	use code_moniker_core::core::shape::Shape;
	predicate::known_kinds(std::iter::once(&lang))
		.into_iter()
		.map(|k| (k, Shape::for_kind(k.as_bytes())))
		.collect()
}

fn langs_inner<W: Write>(args: &LangsArgs, stdout: &mut W) -> anyhow::Result<()> {
	use code_moniker_core::lang::Lang;

	match &args.lang {
		None => match args.format {
			LangsFormat::Text => {
				for lang in Lang::ALL {
					writeln!(stdout, "{}", lang.tag())?;
				}
			}
			LangsFormat::Json => {
				let tags: Vec<&str> = Lang::ALL.iter().map(|l| l.tag()).collect();
				serde_json::to_writer_pretty(&mut *stdout, &tags)?;
				stdout.write_all(b"\n")?;
			}
		},
		Some(tag) => {
			let lang = Lang::from_tag(tag).ok_or_else(|| {
				let known: Vec<&str> = Lang::ALL.iter().map(|l| l.tag()).collect();
				anyhow::anyhow!("unknown language `{tag}` (known: {})", known.join(", "))
			})?;
			let kinds = collect_kinds(lang);
			let visibilities = lang.allowed_visibilities();
			match args.format {
				LangsFormat::Text => write_langs_text(stdout, lang.tag(), &kinds, visibilities)?,
				LangsFormat::Json => write_langs_json(stdout, lang.tag(), &kinds, visibilities)?,
			}
		}
	}
	Ok(())
}

fn write_langs_text<W: Write>(
	w: &mut W,
	tag: &str,
	kinds: &[(&'static str, code_moniker_core::core::shape::Shape)],
	visibilities: &[&'static str],
) -> std::io::Result<()> {
	use code_moniker_core::core::shape::Shape;
	writeln!(w, "lang: {tag}")?;
	writeln!(w, "kinds:")?;
	let width = Shape::ALL
		.iter()
		.map(|s| s.as_str().len() + 1)
		.max()
		.unwrap_or(0);
	for shape in Shape::ALL {
		let names: Vec<&str> = kinds
			.iter()
			.filter(|(_, s)| s == shape)
			.map(|(n, _)| *n)
			.collect();
		if names.is_empty() {
			continue;
		}
		writeln!(
			w,
			"  {:<width$} {}",
			format!("{}:", shape.as_str()),
			names.join(", "),
			width = width
		)?;
	}
	if visibilities.is_empty() {
		writeln!(w, "visibilities: (none — ignored by this language)")?;
	} else {
		writeln!(w, "visibilities: {}", visibilities.join(", "))?;
	}
	Ok(())
}

fn write_langs_json<W: Write>(
	w: &mut W,
	tag: &str,
	kinds: &[(&'static str, code_moniker_core::core::shape::Shape)],
	visibilities: &[&'static str],
) -> anyhow::Result<()> {
	#[derive(serde::Serialize)]
	struct KindEntry<'a> {
		name: &'a str,
		shape: &'a str,
	}
	#[derive(serde::Serialize)]
	struct Out<'a> {
		lang: &'a str,
		kinds: Vec<KindEntry<'a>>,
		visibilities: &'a [&'static str],
	}
	let out = Out {
		lang: tag,
		kinds: kinds
			.iter()
			.map(|(n, s)| KindEntry {
				name: n,
				shape: s.as_str(),
			})
			.collect(),
		visibilities,
	};
	serde_json::to_writer_pretty(&mut *w, &out)?;
	w.write_all(b"\n")?;
	Ok(())
}

fn run_extract<W1: Write, W2: Write>(args: &ExtractArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
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

fn extract_inner<W: Write>(args: &ExtractArgs, stdout: &mut W) -> anyhow::Result<bool> {
	let path: &Path = &args.path;
	let scheme = args.scheme.as_deref().unwrap_or(DEFAULT_SCHEME).to_string();
	let meta = std::fs::metadata(path)
		.map_err(|e| anyhow::anyhow!("cannot stat {}: {e}", path.display()))?;
	if meta.is_dir() {
		return dir::run(args, stdout, path, &scheme);
	}
	let lang = path_to_lang(path)?;
	let predicates = args.compiled_predicates(&scheme)?;
	let names = predicate::compile_name_filters(&args.name)?;
	let known = predicate::known_kinds(std::iter::once(&lang));
	let unknown = predicate::unknown_kinds(&args.kind, &known);
	if !unknown.is_empty() {
		return Err(unknown_kinds_error(&unknown, &[lang], &known));
	}
	let ctx = extract::Context {
		ts: tsconfig::load(path.parent().unwrap_or_else(|| Path::new("."))),
		project: args.project.clone(),
	};
	let (graph, extracted_source) =
		cache::load_or_extract(path, path, lang, args.cache.as_deref(), &ctx)
			.ok_or_else(|| anyhow::anyhow!("cannot read {}", path.display()))?;
	let source = match extracted_source {
		Some(s) => s,
		None => std::fs::read_to_string(path)
			.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?,
	};
	let matches = predicate::filter(&graph, &predicates, &args.kind, &names, &args.shape);
	let any = !matches.defs.is_empty() || !matches.refs.is_empty();
	match args.mode() {
		OutputMode::Default => match args.format {
			OutputFormat::Text => format::write_text(stdout, &matches, args, &scheme)?,
			OutputFormat::Tsv => format::write_tsv(stdout, &matches, &source, args, &scheme)?,
			OutputFormat::Json => {
				format::write_json(stdout, &matches, &source, args, lang, path, &scheme)?
			}
			#[cfg(feature = "pretty")]
			OutputFormat::Tree => tree::write_tree(stdout, &matches, &source, args, &scheme)?,
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
		Ok(outcome) => {
			if outcome.any_error || (outcome.any_violation && args.format != CheckFormat::CodexHook)
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
		check_project(path, &cfg, args.report)?
	} else {
		match check_one_file(path, &cfg, args.report)? {
			Some(report) => (vec![report], Vec::new()),
			None => {
				return Ok(CheckOutcome {
					any_violation: false,
					any_error: false,
				});
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
		any_violation,
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
	any_violation: bool,
	any_error: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
struct FailedRuleSummary {
	rule_id: String,
	violations: usize,
}

struct ViolationEntry<'a> {
	path: &'a Path,
	violation: &'a check::Violation,
}

fn check_one_file(
	path: &Path,
	cfg: &check::Config,
	report: bool,
) -> anyhow::Result<Option<FileReport>> {
	let Ok(lang) = path_to_lang(path) else {
		return Ok(None);
	};
	let compiled = check::compile_rules(cfg, lang, DEFAULT_SCHEME)?;
	check_one_compiled(path, None, lang, &compiled, report).map(Some)
}

/// `moniker_anchor` overrides the path passed to the extractor — used by
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
	let graph = extract::extract(lang, &source, moniker_anchor.unwrap_or(fs_path));
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
	file: &sources::SourceFile,
	ctx: &extract::Context,
	compiled: &check::CompiledRules,
	report: bool,
) -> anyhow::Result<FileReport> {
	let source = std::fs::read_to_string(&file.path)
		.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", file.path.display()))?;
	let graph = extract::extract_with(file.lang, &source, &file.anchor, ctx);
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
	use rayon::prelude::*;
	use std::collections::HashMap;
	let source_set = sources::discover(&[root.to_path_buf()], None)?;
	let mut compiled: HashMap<code_moniker_core::lang::Lang, check::CompiledRules> = HashMap::new();
	for f in &source_set.files {
		if compiled.contains_key(&f.lang) {
			continue;
		}
		compiled.insert(f.lang, check::compile_rules(cfg, f.lang, DEFAULT_SCHEME)?);
	}
	let outcomes: Vec<Result<FileReport, FileError>> = source_set
		.files
		.par_iter()
		.map(|f| {
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
	let mut total = 0usize;
	let mut files_with = 0usize;
	for r in reports {
		if r.violations.is_empty() {
			continue;
		}
		files_with += 1;
		total += r.violations.len();
	}
	let selected = max_violations.map(|max| largest_violation_group(reports, max));
	if let Some(selected) = &selected {
		if let Some(first) = selected.first() {
			writeln!(
				w,
				"Showing {} of {total} violation(s) from largest rule group `{}`.",
				selected.len(),
				first.violation.rule_id
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
	let single_clean = reports.len() == 1 && files_with == 0 && errors.is_empty();
	if !single_clean {
		write!(
			w,
			"\n{total} violation(s) across {files_with} file(s) ({} scanned, elapsed {} ms",
			reports.len(),
			duration_ms(elapsed)
		)?;
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
	Ok(())
}

fn largest_violation_group<'a>(reports: &'a [FileReport], max: usize) -> Vec<ViolationEntry<'a>> {
	use std::collections::BTreeMap;
	let mut by_rule: BTreeMap<&str, Vec<ViolationEntry<'a>>> = BTreeMap::new();
	for report in reports {
		for violation in &report.violations {
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
		writeln!(w, "- {}: {} violation(s)", item.rule_id, item.violations)?;
	}
	Ok(())
}

fn failed_rule_summary(reports: &[FileReport]) -> Vec<FailedRuleSummary> {
	use std::collections::BTreeMap;
	let mut by_rule: BTreeMap<String, usize> = BTreeMap::new();
	for report in reports {
		for violation in &report.violations {
			*by_rule.entry(violation.rule_id.clone()).or_default() += 1;
		}
	}
	let mut out: Vec<_> = by_rule
		.into_iter()
		.map(|(rule_id, violations)| FailedRuleSummary {
			rule_id,
			violations,
		})
		.collect();
	out.sort_by(|a, b| {
		b.violations
			.cmp(&a.violations)
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
	let any_violation = reports.iter().any(|report| !report.violations.is_empty());
	if !any_violation {
		return Ok(());
	}
	let reason = codex_hook_reason(reports, errors, elapsed, max_violations)?;
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

fn duration_ms(duration: Duration) -> u64 {
	duration.as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn violation(rule_id: &str, line: u32, message: &str) -> check::Violation {
		check::Violation {
			rule_id: rule_id.to_string(),
			moniker: "code+moniker://./lang:rs/module:ui".to_string(),
			kind: "imports_symbol".to_string(),
			lines: (line, line),
			message: message.to_string(),
			explanation: None,
		}
	}

	#[test]
	fn exit_codes_are_stable() {
		assert_eq!(ExitCode::from(Exit::Match), ExitCode::SUCCESS);
		assert_eq!(ExitCode::from(Exit::NoMatch), ExitCode::from(1));
		assert_eq!(ExitCode::from(Exit::UsageError), ExitCode::from(2));
	}

	#[test]
	fn shape_description_exists_for_every_canonical_shape() {
		for shape in code_moniker_core::core::shape::Shape::ALL {
			assert!(
				!shape_description(*shape).is_empty(),
				"missing description for {shape:?}"
			);
		}
	}

	#[test]
	fn max_violations_picks_largest_rule_group_by_path() {
		let reports = vec![
			FileReport {
				path: PathBuf::from("src/b.rs"),
				violations: vec![
					violation("refs.large", 20, "second by path"),
					violation("refs.small", 1, "ignored smaller group"),
				],
				rule_reports: Vec::new(),
			},
			FileReport {
				path: PathBuf::from("src/a.rs"),
				violations: vec![violation("refs.large", 30, "third by line")],
				rule_reports: Vec::new(),
			},
			FileReport {
				path: PathBuf::from("src/a.rs"),
				violations: vec![violation("refs.large", 10, "first by line")],
				rule_reports: Vec::new(),
			},
			FileReport {
				path: PathBuf::from("src/c.rs"),
				violations: vec![
					violation("refs.small", 2, "ignored smaller group"),
					violation("refs.other", 3, "ignored smaller group"),
				],
				rule_reports: Vec::new(),
			},
		];
		let selected = largest_violation_group(&reports, 2);

		assert_eq!(selected.len(), 2);
		assert_eq!(selected[0].path, Path::new("src/a.rs"));
		assert_eq!(selected[0].violation.lines, (10, 10));
		assert_eq!(selected[1].path, Path::new("src/a.rs"));
		assert_eq!(selected[1].violation.lines, (30, 30));
		assert!(
			selected
				.iter()
				.all(|entry| entry.violation.rule_id == "refs.large")
		);
	}

	#[test]
	fn codex_hook_format_emits_block_json_with_rule_diagnostic() {
		let reports = vec![FileReport {
			path: PathBuf::from("src/lib.rs"),
			violations: vec![check::Violation {
				rule_id: "refs.ui-store-boundary".to_string(),
				moniker: "code+moniker://./lang:rs/module:ui".to_string(),
				kind: "imports_symbol".to_string(),
				lines: (11, 11),
				message: "`code-moniker ui` must consume workspace read models".to_string(),
				explanation: Some("raw graph/index/Git records stay inside workspace".to_string()),
			}],
			rule_reports: Vec::new(),
		}];
		let mut out = Vec::new();

		write_reports_codex_hook(&mut out, &reports, &[], Duration::from_millis(12), None).unwrap();

		let feedback: serde_json::Value = serde_json::from_slice(&out).unwrap();
		assert_eq!(feedback["decision"], "block");
		let reason = feedback["reason"].as_str().unwrap();
		assert!(reason.contains("code-moniker architecture check failed"));
		assert!(reason.contains("src/lib.rs:L11-L11 [refs.ui-store-boundary]"));
		assert!(reason.contains("`code-moniker ui` must consume workspace read models"));
		assert!(feedback.get("hookSpecificOutput").is_none());
	}

	#[test]
	fn codex_hook_format_stays_silent_when_clean() {
		let reports = vec![FileReport {
			path: PathBuf::from("src/lib.rs"),
			violations: Vec::new(),
			rule_reports: Vec::new(),
		}];
		let mut out = Vec::new();

		write_reports_codex_hook(&mut out, &reports, &[], Duration::from_millis(12), None).unwrap();

		assert!(out.is_empty());
	}
}
