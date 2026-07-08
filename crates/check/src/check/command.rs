use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use code_moniker_core::core::code_graph::{CodeGraph, DefRecord};
use code_moniker_core::lang::Lang;
use code_moniker_workspace::environment;
use code_moniker_workspace::lang::path_to_lang;

use crate::check;
use crate::check::config::{self, RuleSeverity};
use crate::check::eval::CompiledRuleSpec;
use crate::check::expr::Domain;

/// One scanned file's rule outcome: the suppression-filtered violations and,
/// when `report` is requested, the per-rule observability counts.
#[derive(Clone, Debug)]
pub struct FileReport {
	pub path: PathBuf,
	pub violations: Vec<check::Violation>,
	pub rule_reports: Vec<check::RuleReport>,
}

/// A per-file I/O or extraction failure, accumulated rather than aborting a
/// project scan.
#[derive(Clone, Debug)]
pub struct FileError {
	pub path: PathBuf,
	pub error: String,
}

pub trait CheckWorkspace: Sync {
	fn is_dir(&self, path: &Path) -> anyhow::Result<bool>;
	fn read_to_string(&self, path: &Path) -> anyhow::Result<String>;
	fn source_set(
		&self,
		root: &Path,
		files: &[PathBuf],
	) -> anyhow::Result<environment::SourceFileSet>;
	fn exists(&self, path: &Path) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FsCheckWorkspace;

impl CheckWorkspace for FsCheckWorkspace {
	fn is_dir(&self, path: &Path) -> anyhow::Result<bool> {
		let meta = std::fs::metadata(path)
			.map_err(|e| anyhow::anyhow!("cannot stat {}: {e}", path.display()))?;
		Ok(meta.is_dir())
	}

	fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
		std::fs::read_to_string(path)
			.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))
	}

	fn source_set(
		&self,
		root: &Path,
		files: &[PathBuf],
	) -> anyhow::Result<environment::SourceFileSet> {
		if files.is_empty() {
			environment::discover_sources(&[root.to_path_buf()], None)
		} else {
			environment::discover_source_files(root, files, None)
		}
	}

	fn exists(&self, path: &Path) -> bool {
		path.exists()
	}
}

#[derive(Clone, Debug)]
pub struct MemoryCheckWorkspace {
	root: PathBuf,
	files: BTreeMap<PathBuf, MemorySourceFile>,
}

#[derive(Clone, Debug)]
struct MemorySourceFile {
	body: String,
	lang: Lang,
}

impl MemoryCheckWorkspace {
	pub fn new(root: impl Into<PathBuf>) -> Self {
		Self {
			root: root.into(),
			files: BTreeMap::new(),
		}
	}

	pub fn with_file(
		mut self,
		path: impl Into<PathBuf>,
		body: impl Into<String>,
		lang: Lang,
	) -> Self {
		self.files.insert(
			normalize_relative(path.into()),
			MemorySourceFile {
				body: body.into(),
				lang,
			},
		);
		self
	}

	pub fn root(&self) -> &Path {
		&self.root
	}

	fn rel_path(&self, path: &Path) -> PathBuf {
		normalize_relative(path.strip_prefix(&self.root).unwrap_or(path).to_path_buf())
	}
}

impl CheckWorkspace for MemoryCheckWorkspace {
	fn is_dir(&self, path: &Path) -> anyhow::Result<bool> {
		Ok(path == self.root || path == Path::new("."))
	}

	fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
		let rel = self.rel_path(path);
		self.files
			.get(&rel)
			.map(|file| file.body.clone())
			.ok_or_else(|| anyhow::anyhow!("cannot read {}: not found", path.display()))
	}

	fn source_set(
		&self,
		root: &Path,
		files: &[PathBuf],
	) -> anyhow::Result<environment::SourceFileSet> {
		self.ensure_root(root)?;
		Ok(environment::SourceFileSet {
			roots: vec![memory_source_root(&self.root)],
			files: memory_source_files(&self.root, &self.files, files),
			multi: false,
		})
	}

	fn exists(&self, path: &Path) -> bool {
		let rel = self.rel_path(path);
		self.files.contains_key(&rel)
	}
}

impl MemoryCheckWorkspace {
	fn ensure_root(&self, root: &Path) -> anyhow::Result<()> {
		if root == self.root {
			return Ok(());
		}
		anyhow::bail!(
			"memory workspace root mismatch: expected {}, got {}",
			self.root.display(),
			root.display()
		);
	}
}

fn memory_source_root(root: &Path) -> environment::SourceRoot {
	environment::SourceRoot {
		input: root.to_path_buf(),
		path: root.to_path_buf(),
		label: ".".to_string(),
		ctx: environment::ExtractContext::default(),
	}
}

fn memory_source_files(
	root: &Path,
	files: &BTreeMap<PathBuf, MemorySourceFile>,
	requested: &[PathBuf],
) -> Vec<environment::SourceFile> {
	files
		.iter()
		.filter(|(path, _)| memory_file_selected(root, path, requested))
		.map(|(rel_path, file)| environment::SourceFile {
			source: 0,
			path: root.join(rel_path),
			rel_path: rel_path.clone(),
			anchor: rel_path.clone(),
			lang: file.lang,
		})
		.collect()
}

fn memory_file_selected(root: &Path, path: &Path, requested: &[PathBuf]) -> bool {
	requested.is_empty()
		|| requested.iter().any(|candidate| {
			let candidate = normalize_relative(candidate.clone());
			candidate == path || normalize_relative(root.join(&candidate)) == path
		})
}

/// How a consumer wants embedded default rules to participate in a ruleset.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DefaultRulesSelection {
	#[default]
	Config,
	Enabled,
	Disabled,
}

impl DefaultRulesSelection {
	pub fn from_override(value: Option<bool>) -> Self {
		match value {
			Some(true) => Self::Enabled,
			Some(false) => Self::Disabled,
			None => Self::Config,
		}
	}

	pub fn as_override(self) -> Option<bool> {
		match self {
			Self::Config => None,
			Self::Enabled => Some(true),
			Self::Disabled => Some(false),
		}
	}
}

/// Ruleset construction contract shared by CLI, MCP, views, and harnesses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleSetRequest {
	pub rules: Option<PathBuf>,
	pub inline_rules: Vec<String>,
	pub default_rules: DefaultRulesSelection,
	pub profile: Option<String>,
	pub scheme: String,
}

impl RuleSetRequest {
	pub fn new(rules: Option<PathBuf>, scheme: impl Into<String>) -> Self {
		Self {
			rules,
			inline_rules: Vec::new(),
			default_rules: DefaultRulesSelection::Config,
			profile: None,
			scheme: scheme.into(),
		}
	}

	pub fn with_rules(rules: impl Into<PathBuf>, scheme: impl Into<String>) -> Self {
		Self::new(Some(rules.into()), scheme)
	}

	pub fn with_default_rules(mut self, default_rules: DefaultRulesSelection) -> Self {
		self.default_rules = default_rules;
		self
	}

	pub fn with_inline_rules(mut self, inline_rules: Vec<String>) -> Self {
		self.inline_rules = inline_rules;
		self
	}

	pub fn with_profile(mut self, profile: Option<String>) -> Self {
		self.profile = profile;
		self
	}

	pub fn rules_path(&self) -> Option<&Path> {
		self.rules.as_deref()
	}

	pub fn scheme(&self) -> &str {
		&self.scheme
	}

	pub fn load_config(&self) -> anyhow::Result<check::Config> {
		let mut cfg = config::load_with_cli_sources(
			self.rules_path(),
			&self.inline_rules,
			self.default_rules.as_override(),
		)?;
		if let Some(profile) = &self.profile {
			cfg.apply_profile(profile)?;
		}
		Ok(cfg)
	}

	pub fn compiled_specs_for_langs(
		&self,
		langs: impl IntoIterator<Item = Lang>,
	) -> anyhow::Result<Vec<CompiledRuleSpec>> {
		let cfg = self.load_config()?;
		compiled_specs_with_config(&cfg, langs, &self.scheme)
	}

	pub fn check_source(
		&self,
		source: &str,
		anchor: &Path,
		lang: Lang,
		report: bool,
	) -> anyhow::Result<SourceReport> {
		let cfg = self.load_config()?;
		check_source_with_config(&cfg, source, anchor, lang, &self.scheme, report)
	}
}

/// Executable check request over either a file, a project root, or a filtered
/// set of project-relative files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckRequest {
	pub path: PathBuf,
	pub rules: RuleSetRequest,
	pub report: bool,
	pub files: Vec<PathBuf>,
}

impl CheckRequest {
	pub fn new(path: impl Into<PathBuf>, rules: RuleSetRequest) -> Self {
		Self {
			path: path.into(),
			rules,
			report: false,
			files: Vec::new(),
		}
	}

	pub fn with_report(mut self, report: bool) -> Self {
		self.report = report;
		self
	}

	pub fn with_files(mut self, files: Vec<PathBuf>) -> Self {
		self.files = files;
		self
	}

	pub fn run(&self) -> anyhow::Result<CheckRun> {
		self.run_with_workspace(&FsCheckWorkspace)
	}

	pub fn run_with_workspace(&self, workspace: &dyn CheckWorkspace) -> anyhow::Result<CheckRun> {
		let started = Instant::now();
		let cfg = self.rules.load_config()?;
		let (reports, errors, skip_reason) = if workspace.is_dir(&self.path)? {
			self.run_directory(&cfg, workspace)?
		} else {
			self.run_single_file(&cfg, workspace)?
		};
		Ok(CheckRun {
			reports,
			errors,
			elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
			skip_reason,
		})
	}

	fn run_directory(
		&self,
		cfg: &check::Config,
		workspace: &dyn CheckWorkspace,
	) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>, Option<CheckSkipReason>)> {
		let (reports, errors) = if self.files.is_empty() {
			check_project_workspace(&self.path, cfg, self.rules.scheme(), self.report, workspace)?
		} else {
			check_project_files_workspace(
				&self.path,
				&self.files,
				cfg,
				self.rules.scheme(),
				self.report,
				workspace,
			)?
		};
		let skip_reason = if !self.files.is_empty() && reports.is_empty() && errors.is_empty() {
			Some(CheckSkipReason::NoMatchingFiles)
		} else {
			None
		};
		Ok((reports, errors, skip_reason))
	}

	fn run_single_file(
		&self,
		cfg: &check::Config,
		workspace: &dyn CheckWorkspace,
	) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>, Option<CheckSkipReason>)> {
		if !self.files.is_empty() {
			anyhow::bail!("--file can only be used when check PATH is a directory");
		}
		let excluded = path_excluded(&self.path, cfg);
		match check_one_file_workspace(
			&self.path,
			cfg,
			self.rules.scheme(),
			self.report,
			workspace,
		)? {
			Some(report) => Ok((vec![report], Vec::new(), None)),
			None if excluded => Ok((
				Vec::new(),
				Vec::new(),
				Some(CheckSkipReason::ExcludedSingleFile),
			)),
			None => Ok((
				Vec::new(),
				Vec::new(),
				Some(CheckSkipReason::UnsupportedSingleFile),
			)),
		}
	}
}

/// Empty-scan reason. Renderers use it to preserve silent text hooks while
/// still allowing structured JSON for intentionally empty scans.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckSkipReason {
	ExcludedSingleFile,
	UnsupportedSingleFile,
	NoMatchingFiles,
}

/// Structured result of a check request. It contains no terminal formatting or
/// process exit policy.
#[derive(Clone, Debug)]
pub struct CheckRun {
	pub reports: Vec<FileReport>,
	pub errors: Vec<FileError>,
	pub elapsed_ms: u64,
	pub skip_reason: Option<CheckSkipReason>,
}

impl CheckRun {
	pub fn any_error_violation(&self) -> bool {
		self.reports.iter().any(|report| {
			report
				.violations
				.iter()
				.any(|violation| violation.severity.is_error())
		})
	}

	pub fn any_error(&self) -> bool {
		!self.errors.is_empty()
	}

	pub fn violation_counts(&self) -> ViolationCounts {
		violation_counts(&self.reports)
	}

	pub fn summary(&self) -> CheckSummary {
		let counts = self.violation_counts();
		CheckSummary {
			files_scanned: self.reports.len(),
			files_with_violations: counts.files_with,
			total_violations: counts.total,
			total_rule_errors: counts.errors,
			total_warnings: counts.warnings,
			files_with_errors: self.errors.len(),
			total_errors: self.errors.len(),
			elapsed_ms: self.elapsed_ms,
			failed_rules: self.failed_rule_summary(),
		}
	}

	pub fn failed_rule_summary(&self) -> Vec<FailedRuleSummary> {
		failed_rule_summary(&self.reports)
	}

	pub fn file_violations(&self) -> impl Iterator<Item = (&Path, &check::eval::Violation)> {
		self.reports.iter().flat_map(|report| {
			report
				.violations
				.iter()
				.map(move |violation| (report.path.as_path(), violation))
		})
	}

	pub fn error_summaries(&self) -> impl Iterator<Item = (&Path, &str)> {
		self.errors
			.iter()
			.map(|error| (error.path.as_path(), error.error.as_str()))
	}

	pub fn rule_violation_totals(&self) -> std::collections::BTreeMap<&str, usize> {
		let mut totals = std::collections::BTreeMap::new();
		for report in &self.reports {
			for rule in &report.rule_reports {
				*totals.entry(rule.rule_id.as_str()).or_insert(0usize) += rule.violations;
			}
		}
		totals
	}
}

/// Serializable aggregate counters for renderers and machine consumers.
#[derive(Clone, Debug, serde::Serialize)]
pub struct CheckSummary {
	pub files_scanned: usize,
	pub files_with_violations: usize,
	pub total_violations: usize,
	pub total_rule_errors: usize,
	pub total_warnings: usize,
	pub files_with_errors: usize,
	pub total_errors: usize,
	pub elapsed_ms: u64,
	pub failed_rules: Vec<FailedRuleSummary>,
}

/// Per-rule failure count, sorted by severity and volume by [`CheckRun`].
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct FailedRuleSummary {
	pub rule_id: String,
	pub severity: RuleSeverity,
	pub violations: usize,
}

/// Count of suppression-filtered violations in a check result.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViolationCounts {
	pub total: usize,
	pub errors: usize,
	pub warnings: usize,
	pub files_with: usize,
}

/// Rules and violations from evaluating one in-memory source.
#[derive(Clone, Debug)]
pub struct SourceReport {
	pub rules: Vec<CompiledRuleSpec>,
	pub violations: Vec<check::Violation>,
	pub rule_reports: Vec<check::RuleReport>,
}

pub fn check_source_with_config(
	cfg: &check::Config,
	source: &str,
	anchor: &Path,
	lang: Lang,
	scheme: &str,
	report: bool,
) -> anyhow::Result<SourceReport> {
	let graph = environment::extract_source_with(
		lang,
		source,
		anchor,
		&environment::ExtractContext::default(),
	);
	check_graph_with_config(cfg, &graph, source, lang, scheme, report)
}

pub fn check_graph_with_config(
	cfg: &check::Config,
	graph: &CodeGraph,
	source: &str,
	lang: Lang,
	scheme: &str,
	report: bool,
) -> anyhow::Result<SourceReport> {
	let compiled = check::compile_rules(cfg, lang, scheme)?;
	let raw = check::evaluate_compiled(graph, source, lang, scheme, &compiled);
	let violations = check::apply_suppressions(graph, source, raw);
	let rule_reports = if report {
		let mut rule_reports = check::rule_report_compiled(graph, source, lang, scheme, &compiled);
		align_report_violations_with_suppressions(&mut rule_reports, &violations);
		rule_reports
	} else {
		Vec::new()
	};
	Ok(SourceReport {
		rules: compiled.specs(lang),
		violations,
		rule_reports,
	})
}

pub fn compiled_specs_with_config(
	cfg: &check::Config,
	langs: impl IntoIterator<Item = Lang>,
	scheme: &str,
) -> anyhow::Result<Vec<CompiledRuleSpec>> {
	let mut specs = Vec::new();
	for lang in langs {
		let compiled = check::compile_rules(cfg, lang, scheme)?;
		specs.extend(compiled.specs(lang));
	}
	specs.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
	Ok(specs)
}

pub fn check_one_file(
	path: &Path,
	cfg: &check::Config,
	scheme: &str,
	report: bool,
) -> anyhow::Result<Option<FileReport>> {
	check_one_file_workspace(path, cfg, scheme, report, &FsCheckWorkspace)
}

pub fn check_one_file_workspace(
	path: &Path,
	cfg: &check::Config,
	scheme: &str,
	report: bool,
	workspace: &dyn CheckWorkspace,
) -> anyhow::Result<Option<FileReport>> {
	let Ok(lang) = path_to_lang(path) else {
		return Ok(None);
	};
	let excludes = check::UriExclusionMatcher::new(&cfg.exclude.uris);
	if excludes.matches_path(path) {
		return Ok(None);
	}
	let compiled = check::compile_rules(cfg, lang, scheme)?;
	let ctx = CompiledCheck {
		scheme,
		compiled: &compiled,
		report,
		workspace,
		requirements: None,
	};
	check_one_compiled(path, None, lang, &ctx).map(Some)
}

struct CompiledCheck<'a> {
	scheme: &'a str,
	compiled: &'a check::CompiledRules,
	report: bool,
	workspace: &'a dyn CheckWorkspace,
	requirements: Option<&'a dyn check::RequirementResolver>,
}

/// `moniker_anchor` overrides the path passed to the extractor - used by
/// project mode to anchor each file's moniker on its path relative to the
/// scan root. `None` means "same as `fs_path`" (single-file mode).
fn check_one_compiled(
	fs_path: &Path,
	moniker_anchor: Option<&Path>,
	lang: code_moniker_core::lang::Lang,
	ctx: &CompiledCheck<'_>,
) -> anyhow::Result<FileReport> {
	let source = ctx.workspace.read_to_string(fs_path)?;
	let graph = environment::extract_source_with(
		lang,
		&source,
		moniker_anchor.unwrap_or(fs_path),
		&environment::ExtractContext::default(),
	);
	let raw = check::evaluate_compiled(&graph, &source, lang, ctx.scheme, ctx.compiled);
	let violations = check::apply_suppressions(&graph, &source, raw);
	let rule_reports = if ctx.report {
		let mut rule_reports =
			check::rule_report_compiled(&graph, &source, lang, ctx.scheme, ctx.compiled);
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
	check_ctx: &CompiledCheck<'_>,
) -> anyhow::Result<FileReport> {
	let source = check_ctx.workspace.read_to_string(&file.path)?;
	let graph = environment::extract_source_with(file.lang, &source, &file.anchor, ctx);
	let raw = check::evaluate_compiled_with_requirements(
		&graph,
		&source,
		file.lang,
		check_ctx.scheme,
		check_ctx.compiled,
		check_ctx.requirements,
	);
	let violations = check::apply_suppressions(&graph, &source, raw);
	let rule_reports = if check_ctx.report {
		let mut rule_reports = check::rule_report_compiled_with_requirements(
			&graph,
			&source,
			file.lang,
			check_ctx.scheme,
			check_ctx.compiled,
			check_ctx.requirements,
		);
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
pub fn check_project(
	root: &Path,
	cfg: &check::Config,
	scheme: &str,
	report: bool,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	check_project_workspace(root, cfg, scheme, report, &FsCheckWorkspace)
}

pub fn check_project_workspace(
	root: &Path,
	cfg: &check::Config,
	scheme: &str,
	report: bool,
	workspace: &dyn CheckWorkspace,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	let source_set = workspace.source_set(root, &[])?;
	let requirements = FileRequirementResolver::new(
		root.to_path_buf(),
		Some(filtered_source_set(&source_set, cfg)),
		workspace,
	);
	check_source_set(
		&source_set,
		cfg,
		scheme,
		report,
		Some(&requirements),
		workspace,
	)
}

pub fn check_project_files(
	root: &Path,
	files: &[PathBuf],
	cfg: &check::Config,
	scheme: &str,
	report: bool,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	check_project_files_workspace(root, files, cfg, scheme, report, &FsCheckWorkspace)
}

pub fn check_project_files_workspace(
	root: &Path,
	files: &[PathBuf],
	cfg: &check::Config,
	scheme: &str,
	report: bool,
	workspace: &dyn CheckWorkspace,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	let source_set = workspace.source_set(root, files)?;
	let resolver_source_set = workspace.source_set(root, &[])?;
	let requirements = FileRequirementResolver::new(
		root.to_path_buf(),
		Some(filtered_source_set(&resolver_source_set, cfg)),
		workspace,
	);
	check_source_set(
		&source_set,
		cfg,
		scheme,
		report,
		Some(&requirements),
		workspace,
	)
}

fn filtered_source_set(
	source_set: &environment::SourceFileSet,
	cfg: &check::Config,
) -> environment::SourceFileSet {
	let excludes = check::UriExclusionMatcher::new(&cfg.exclude.uris);
	environment::SourceFileSet {
		roots: source_set.roots.clone(),
		files: source_set
			.files
			.iter()
			.filter(|file| !excludes.matches_path(&file.path))
			.cloned()
			.collect(),
		multi: source_set.multi,
	}
}

fn check_source_set(
	source_set: &environment::SourceFileSet,
	cfg: &check::Config,
	scheme: &str,
	report: bool,
	requirements: Option<&dyn check::RequirementResolver>,
	workspace: &dyn CheckWorkspace,
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
		compiled.insert(f.lang, check::compile_rules(cfg, f.lang, scheme)?);
	}
	let outcomes: Vec<Result<FileReport, FileError>> = files
		.par_iter()
		.map(|f| {
			let f = *f;
			let rules = &compiled[&f.lang];
			let ctx = &source_set.roots[f.source].ctx;
			let check_ctx = CompiledCheck {
				scheme,
				compiled: rules,
				report,
				workspace,
				requirements,
			};
			check_source_file_compiled(f, ctx, &check_ctx).map_err(|e| FileError {
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

struct FileRequirementResolver<'a> {
	root: PathBuf,
	source_set: Option<environment::SourceFileSet>,
	workspace_defs: OnceLock<Vec<DefRecord>>,
	workspace: &'a dyn CheckWorkspace,
}

impl<'a> FileRequirementResolver<'a> {
	fn new(
		root: PathBuf,
		source_set: Option<environment::SourceFileSet>,
		workspace: &'a dyn CheckWorkspace,
	) -> Self {
		Self {
			root,
			source_set,
			workspace_defs: OnceLock::new(),
			workspace,
		}
	}
}

impl check::RequirementResolver for FileRequirementResolver<'_> {
	fn exists(&self, pattern: &str, _source: &DefRecord, _scheme: &str) -> bool {
		let Some(candidates) = source_candidates_from_requirement(&self.root, pattern) else {
			return false;
		};
		let Ok(path_pattern) = check::path::parse(pattern) else {
			return false;
		};
		for path in candidates {
			if !self.workspace.exists(&path) {
				continue;
			}
			let Ok(lang) = path_to_lang(&path) else {
				continue;
			};
			let Ok(source) = self.workspace.read_to_string(&path) else {
				continue;
			};
			let graph = environment::extract_source_with(
				lang,
				&source,
				&anchor_for_requirement(&self.root, &path),
				&environment::ExtractContext::default(),
			);
			if graph
				.defs()
				.any(|def| check::path::matches(&path_pattern, &def.moniker))
			{
				return true;
			}
		}
		false
	}

	fn descendant_defs<'a>(&'a self, owner: &DefRecord, inner: &Domain) -> Vec<&'a DefRecord> {
		self.workspace_defs()
			.iter()
			.filter(|def| {
				def.moniker != owner.moniker
					&& owner.moniker.is_ancestor_of(&def.moniker)
					&& lazy_domain_matches(inner, def)
			})
			.collect()
	}
}

impl FileRequirementResolver<'_> {
	fn workspace_defs(&self) -> &[DefRecord] {
		self.workspace_defs
			.get_or_init(|| collect_workspace_defs(self.source_set.as_ref(), self.workspace))
	}
}

fn collect_workspace_defs(
	source_set: Option<&environment::SourceFileSet>,
	workspace: &dyn CheckWorkspace,
) -> Vec<DefRecord> {
	let Some(source_set) = source_set else {
		return Vec::new();
	};
	let mut defs = Vec::new();
	for file in &source_set.files {
		let Ok(source) = workspace.read_to_string(&file.path) else {
			continue;
		};
		let ctx = &source_set.roots[file.source].ctx;
		let graph = environment::extract_source_with(file.lang, &source, &file.anchor, ctx);
		defs.extend(graph.defs().cloned());
	}
	defs
}

fn normalize_relative(path: PathBuf) -> PathBuf {
	path.components()
		.filter_map(|component| match component {
			std::path::Component::Normal(part) => Some(PathBuf::from(part)),
			std::path::Component::CurDir => None,
			_ => None,
		})
		.collect()
}

fn lazy_domain_matches(domain: &Domain, def: &DefRecord) -> bool {
	match domain {
		Domain::Children(kind) => def.kind.as_ref() == kind.as_bytes(),
		Domain::ChildrenByShape(shape) => {
			def.shape().is_some_and(|actual| actual.as_str() == shape)
		}
		Domain::Descendants(inner) => lazy_domain_matches(inner, def),
		Domain::Pairs(_) | Domain::Segments | Domain::OutRefs | Domain::InRefs => false,
	}
}

fn source_candidates_from_requirement(root: &Path, pattern: &str) -> Option<Vec<PathBuf>> {
	let mut dirs = Vec::new();
	let mut module = None;
	for step in pattern.split('/') {
		if let Some(dir) = literal_step_name(step, "dir") {
			dirs.push(dir.to_string());
		} else if let Some(name) = literal_step_name(step, "module") {
			module = Some(name.to_string());
		}
	}
	let module = module?;
	let base = dirs
		.iter()
		.fold(root.to_path_buf(), |path, dir| path.join(dir));
	if module == "mod" {
		Some(vec![base.join("mod.rs")])
	} else {
		Some(vec![
			base.join(format!("{module}.rs")),
			base.join(module).join("mod.rs"),
		])
	}
}

fn literal_step_name<'a>(step: &'a str, kind: &str) -> Option<&'a str> {
	let (step_kind, name) = step.split_once(':')?;
	(step_kind == kind && !name.contains(['*', '{', '}', '/'])).then_some(name)
}

fn anchor_for_requirement(root: &Path, path: &Path) -> PathBuf {
	path.strip_prefix(root).unwrap_or(path).to_path_buf()
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

fn path_excluded(path: &Path, cfg: &check::Config) -> bool {
	check::UriExclusionMatcher::new(&cfg.exclude.uris).matches_path(path)
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

fn failed_rule_summary(reports: &[FileReport]) -> Vec<FailedRuleSummary> {
	use std::collections::BTreeMap;
	let mut by_rule: BTreeMap<(String, RuleSeverity), usize> = BTreeMap::new();
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
