use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use code_moniker_core::core::code_graph::{CodeGraph, DefRecord};
use code_moniker_core::lang::Lang;
use code_moniker_workspace::code::{LocalCodeIndex, LocalCodeIndexOptions};
use code_moniker_workspace::environment;
use code_moniker_workspace::lang::path_to_lang;
use code_moniker_workspace::linkage::{LinkagePort, LocalLinkage};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{
	ChangeOverlay, RecordTable, ResourceGeneration, SourceFileRecord, SourceId,
	SymbolInventoryIndex, SymbolSet, WorkspaceRequest, WorkspaceSnapshot, WorkspaceTimings,
	WorkspaceTransition,
};
use code_moniker_workspace::source::{
	CodeIndexMaterial, IndexedSourceFile, LocalIdentityResolver, LocalResourceCache,
};

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
	fn source_graph(
		&self,
		file: &environment::SourceFile,
		ctx: &environment::ExtractContext,
	) -> anyhow::Result<(String, CodeGraph)> {
		let source = self.read_to_string(&file.path)?;
		let graph = environment::extract_source_with(file.lang, &source, &file.anchor, ctx);
		Ok((source, graph))
	}
	fn source_set(
		&self,
		root: &Path,
		files: &[PathBuf],
	) -> anyhow::Result<environment::SourceFileSet>;
	fn source_catalog(&self, root: &Path) -> anyhow::Result<environment::SourceFileSet>;
	fn exists(&self, path: &Path) -> bool;
	fn linked_snapshot(
		&self,
		_source_set: &environment::SourceFileSet,
		_scheme: &str,
	) -> anyhow::Result<Option<Arc<WorkspaceSnapshot>>>;
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

	fn source_catalog(&self, root: &Path) -> anyhow::Result<environment::SourceFileSet> {
		environment::discover_source_catalog(root, None)
	}

	fn exists(&self, path: &Path) -> bool {
		path.exists()
	}

	fn linked_snapshot(
		&self,
		source_set: &environment::SourceFileSet,
		scheme: &str,
	) -> anyhow::Result<Option<Arc<WorkspaceSnapshot>>> {
		build_fs_linked_snapshot(source_set, scheme).map(Some)
	}
}

fn build_fs_linked_snapshot(
	source_set: &environment::SourceFileSet,
	scheme: &str,
) -> anyhow::Result<Arc<WorkspaceSnapshot>> {
	let paths = source_set
		.roots
		.iter()
		.map(|root| root.input.clone())
		.collect();
	let options =
		LocalWorkspaceOptions::new(paths, None).with_identity(LocalIdentityResolver::new(scheme));
	let mut registry = LocalWorkspaceRegistry::local(options);
	match registry
		.commands()
		.refresh(WorkspaceRequest::new("check-workspace-linkage"))
	{
		WorkspaceTransition::Ready { .. } => registry
			.queries()
			.snapshot_arc()
			.ok_or_else(|| anyhow::anyhow!("workspace refresh completed without a snapshot")),
		WorkspaceTransition::Failed { failure, .. } => {
			anyhow::bail!("workspace linkage build failed: {}", failure.message)
		}
	}
}

#[derive(Clone)]
pub struct IndexedCheckWorkspace {
	root: PathBuf,
	material: Arc<CodeIndexMaterial>,
	snapshot: Arc<WorkspaceSnapshot>,
}

impl IndexedCheckWorkspace {
	pub fn from_snapshot(
		root: impl Into<PathBuf>,
		cache: &LocalResourceCache,
		snapshot: Arc<WorkspaceSnapshot>,
	) -> anyhow::Result<Self> {
		let material = indexed_material_for_snapshot(cache, &snapshot)?;
		Ok(Self {
			root: root.into(),
			material,
			snapshot,
		})
	}
}

fn indexed_material_for_snapshot(
	cache: &LocalResourceCache,
	snapshot: &WorkspaceSnapshot,
) -> anyhow::Result<Arc<CodeIndexMaterial>> {
	if snapshot.linkage.index_generation != snapshot.index.generation {
		anyhow::bail!(
			"indexed snapshot generation mismatch: linkage uses {}, index uses {}",
			snapshot.linkage.index_generation.value(),
			snapshot.index.generation.value()
		);
	}
	let material = cache
		.index_material(snapshot.index.generation)
		.ok_or_else(|| {
			anyhow::anyhow!(
				"indexed source material is unavailable for generation {}",
				snapshot.index.generation.value()
			)
		})?;
	if material.identity.scheme() != snapshot.index.identity_scheme {
		anyhow::bail!(
			"indexed snapshot scheme mismatch: material uses {}, index uses {}",
			material.identity.scheme(),
			snapshot.index.identity_scheme
		);
	}
	Ok(material)
}

fn indexed_file<'a>(
	material: &'a CodeIndexMaterial,
	root: &Path,
	path: &Path,
) -> Option<&'a IndexedSourceFile> {
	let absolute = if path.is_absolute() {
		path.to_path_buf()
	} else {
		root.join(path)
	};
	material.files.iter().find_map(|file| {
		(file.path == path || file.path == absolute || file.rel_path == path || file.anchor == path)
			.then_some(file.as_ref())
	})
}

fn indexed_source_file<'a>(
	material: &'a CodeIndexMaterial,
	file: &environment::SourceFile,
) -> Option<&'a IndexedSourceFile> {
	let file_idx = material.source_set().files.iter().position(|candidate| {
		candidate.source == file.source
			&& candidate.path == file.path
			&& candidate.rel_path == file.rel_path
			&& candidate.anchor == file.anchor
			&& candidate.lang == file.lang
	})?;
	let indexed = material.files.get(file_idx)?;
	(indexed.source_root == file.source).then_some(indexed)
}

fn indexed_file_selected(
	source_set: &environment::SourceFileSet,
	root: &Path,
	file: &environment::SourceFile,
	requested: &[PathBuf],
) -> bool {
	if file.retired
		|| source_set
			.roots
			.get(file.source)
			.is_none_or(|source_root| source_root.input != root)
	{
		return false;
	}
	if requested.is_empty() {
		return true;
	}
	requested.iter().any(|path| {
		let absolute = if path.is_absolute() {
			path.clone()
		} else {
			root.join(path)
		};
		file.path == *path
			|| file.path == absolute
			|| file.rel_path == *path
			|| file.anchor == *path
	})
}

fn ensure_indexed_root(expected: &Path, actual: &Path) -> anyhow::Result<()> {
	if actual == expected {
		return Ok(());
	}
	anyhow::bail!(
		"indexed workspace root mismatch: expected {}, got {}",
		expected.display(),
		actual.display()
	);
}

impl CheckWorkspace for IndexedCheckWorkspace {
	fn is_dir(&self, path: &Path) -> anyhow::Result<bool> {
		Ok(path == self.root)
	}

	fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
		indexed_file(&self.material, &self.root, path)
			.map(|file| file.source.clone())
			.ok_or_else(|| anyhow::anyhow!("cannot read {}: not indexed", path.display()))
	}

	fn source_graph(
		&self,
		file: &environment::SourceFile,
		_ctx: &environment::ExtractContext,
	) -> anyhow::Result<(String, CodeGraph)> {
		let indexed = indexed_source_file(&self.material, file)
			.ok_or_else(|| anyhow::anyhow!("cannot read {}: not indexed", file.path.display()))?;
		Ok((indexed.source.clone(), indexed.graph.clone()))
	}

	fn source_set(
		&self,
		root: &Path,
		files: &[PathBuf],
	) -> anyhow::Result<environment::SourceFileSet> {
		ensure_indexed_root(&self.root, root)?;
		let source_set = self.material.source_set();
		Ok(environment::SourceFileSet {
			roots: source_set.roots.clone(),
			files: source_set
				.files
				.iter()
				.filter(|file| indexed_file_selected(source_set, &self.root, file, files))
				.cloned()
				.collect(),
			multi: source_set.multi,
		})
	}

	fn source_catalog(&self, root: &Path) -> anyhow::Result<environment::SourceFileSet> {
		self.source_set(root, &[])
	}

	fn exists(&self, path: &Path) -> bool {
		indexed_file(&self.material, &self.root, path).is_some()
	}

	fn linked_snapshot(
		&self,
		_source_set: &environment::SourceFileSet,
		scheme: &str,
	) -> anyhow::Result<Option<Arc<WorkspaceSnapshot>>> {
		if scheme != self.snapshot.index.identity_scheme {
			anyhow::bail!(
				"indexed workspace scheme mismatch: expected {}, got {scheme}",
				self.snapshot.index.identity_scheme
			);
		}
		Ok(Some(Arc::clone(&self.snapshot)))
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
}

impl CheckWorkspace for MemoryCheckWorkspace {
	fn is_dir(&self, path: &Path) -> anyhow::Result<bool> {
		Ok(path == self.root || path == Path::new("."))
	}

	fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
		let rel = memory_rel_path(&self.root, path);
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
		ensure_memory_root(&self.root, root)?;
		Ok(environment::SourceFileSet {
			roots: vec![memory_source_root(&self.root)],
			files: memory_source_files(&self.root, &self.files, files),
			multi: false,
		})
	}

	fn source_catalog(&self, root: &Path) -> anyhow::Result<environment::SourceFileSet> {
		self.source_set(root, &[])
	}

	fn exists(&self, path: &Path) -> bool {
		let rel = memory_rel_path(&self.root, path);
		self.files.contains_key(&rel)
	}

	fn linked_snapshot(
		&self,
		source_set: &environment::SourceFileSet,
		scheme: &str,
	) -> anyhow::Result<Option<Arc<WorkspaceSnapshot>>> {
		build_memory_linked_snapshot(self, source_set, scheme).map(Some)
	}
}

fn ensure_memory_root(expected: &Path, actual: &Path) -> anyhow::Result<()> {
	if actual == expected {
		return Ok(());
	}
	anyhow::bail!(
		"memory workspace root mismatch: expected {}, got {}",
		expected.display(),
		actual.display()
	);
}

fn build_memory_linked_snapshot(
	workspace: &MemoryCheckWorkspace,
	source_set: &environment::SourceFileSet,
	scheme: &str,
) -> anyhow::Result<Arc<WorkspaceSnapshot>> {
	let identity = LocalIdentityResolver::new(scheme);
	let files = memory_indexed_files(workspace, source_set, &identity)?;
	let cache = LocalResourceCache::default();
	let mut code_index = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone());
	let (catalog, index) = code_index
		.build_index_from_extracted(source_set.clone(), identity, files)
		.map_err(|failure| anyhow::anyhow!(failure.message))?;
	let mut linker = LocalLinkage::new(cache);
	let linkage = linker
		.resolve_linkage(&index)
		.map_err(|failure| anyhow::anyhow!(failure.message))?;
	let changes = ChangeOverlay::new(
		catalog.generation,
		catalog.generation,
		index.generation,
		Vec::new(),
	);
	Ok(Arc::new(WorkspaceSnapshot {
		generation: linkage.generation,
		catalog,
		index,
		linkage,
		changes,
		timings: WorkspaceTimings::default(),
	}))
}

fn memory_indexed_files(
	workspace: &MemoryCheckWorkspace,
	source_set: &environment::SourceFileSet,
	identity: &LocalIdentityResolver,
) -> anyhow::Result<Vec<IndexedSourceFile>> {
	source_set
		.files
		.iter()
		.enumerate()
		.map(|(file_idx, file)| {
			let source = workspace
				.files
				.get(&file.rel_path)
				.ok_or_else(|| anyhow::anyhow!("cannot read {}: not found", file.path.display()))?;
			let root = source_set
				.roots
				.get(file.source)
				.ok_or_else(|| anyhow::anyhow!("source root {} is unavailable", file.source))?;
			let ctx = file.extraction_context(root);
			Ok(IndexedSourceFile {
				source_root: file.source,
				source_id: identity.source_id(file_idx, &file.rel_path),
				source_uri: identity.source_uri(&file.rel_path),
				identity: LocalIdentityResolver::new(identity.scheme()),
				path: file.path.to_path_buf(),
				rel_path: file.rel_path.to_path_buf(),
				anchor: file.anchor.to_path_buf(),
				lang: file.lang,
				graph: environment::extract_source_with(
					file.lang,
					&source.body,
					&file.anchor,
					&ctx,
				),
				source: source.body.to_owned(),
				extraction_cache: "provided",
				extraction_duration: std::time::Duration::ZERO,
			})
		})
		.collect()
}

fn memory_rel_path(root: &Path, path: &Path) -> PathBuf {
	normalize_relative(path.strip_prefix(root).unwrap_or(path).to_path_buf())
}

fn memory_source_root(root: &Path) -> environment::SourceRoot {
	environment::SourceRoot {
		input: root.to_path_buf(),
		path: root.to_path_buf(),
		label: ".".to_string(),
		ctx: environment::ExtractContext::default(),
		source_groups: Default::default(),
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
			root_moniker: environment::source_root_moniker(
				file.lang,
				rel_path,
				&environment::ExtractContext::default(),
			),
			source_group: None,
			srcset: None,
			retired: false,
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

/// Ruleset construction contract shared by CLI, MCP, views, and agent integrations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleSetRequest {
	pub rules: Option<PathBuf>,
	pub inline_rules: Vec<String>,
	pub default_rules: DefaultRulesSelection,
	pub profile: Option<String>,
	pub scheme: String,
	pub project_root: Option<PathBuf>,
}

impl RuleSetRequest {
	pub fn new(rules: Option<PathBuf>, scheme: impl Into<String>) -> Self {
		Self {
			rules,
			inline_rules: Vec::new(),
			default_rules: DefaultRulesSelection::Config,
			profile: None,
			scheme: scheme.into(),
			project_root: None,
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

	pub fn with_project_root(mut self, project_root: impl Into<PathBuf>) -> Self {
		self.project_root = Some(project_root.into());
		self
	}

	pub fn rules_path(&self) -> Option<&Path> {
		self.rules.as_deref()
	}

	pub fn scheme(&self) -> &str {
		&self.scheme
	}

	pub fn load_config(&self) -> anyhow::Result<check::Config> {
		let mut cfg = if let Some(project_root) = &self.project_root {
			config::load_project_with_cli_sources(
				project_root,
				self.rules_path(),
				&self.inline_rules,
				self.default_rules.as_override(),
			)?
		} else {
			config::load_with_cli_sources(
				self.rules_path(),
				&self.inline_rules,
				self.default_rules.as_override(),
			)?
		};
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
		let path = path.into();
		Self {
			rules: rules.with_project_root(path.clone()),
			path,
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
			violations_by_srcset: self.violations_by_srcset(),
		}
	}

	pub fn failed_rule_summary(&self) -> Vec<FailedRuleSummary> {
		failed_rule_summary(&self.reports)
	}

	pub fn violations_by_srcset(&self) -> std::collections::BTreeMap<String, usize> {
		violations_by_srcset(&self.reports)
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
	#[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
	pub violations_by_srcset: std::collections::BTreeMap<String, usize>,
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
	let mut specs = crate::check::workspace_eval::compile_workspace_rules(cfg, scheme)?.specs();
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

struct CheckedSourceFile {
	file: environment::SourceFile,
	source: String,
	graph: CodeGraph,
	report: FileReport,
}

fn check_source_file_compiled(
	file: &environment::SourceFile,
	ctx: &environment::ExtractContext,
	check_ctx: &CompiledCheck<'_>,
) -> anyhow::Result<CheckedSourceFile> {
	let (source, graph) = check_ctx.workspace.source_graph(file, ctx)?;
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
	Ok(CheckedSourceFile {
		file: file.clone(),
		source,
		graph,
		report: FileReport {
			path: file.path.clone(),
			violations,
			rule_reports,
		},
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
		filtered_source_set(&source_set, cfg),
		&cfg.exclude.uris,
		workspace,
	);
	check_source_set(
		&source_set,
		cfg,
		scheme,
		Some(&requirements),
		workspace,
		SourceSetCheckMode {
			report,
			workspace_rules: true,
		},
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
	let requirements =
		FileRequirementResolver::new(root.to_path_buf(), None, &cfg.exclude.uris, workspace);
	let (reports, mut errors) = check_source_set(
		&source_set,
		cfg,
		scheme,
		Some(&requirements),
		workspace,
		SourceSetCheckMode {
			report,
			workspace_rules: false,
		},
	)?;
	if !cfg.workspace.symbol.rules.is_empty()
		|| !cfg.workspace.group.rules.is_empty()
		|| !cfg.workspace.path.is_empty()
	{
		errors.push(FileError {
			path: root.to_path_buf(),
			error: "workspace rules were not run: a file-scoped check does not provide a complete symbol inventory"
				.to_string(),
		});
	}
	if let Some(error) = requirements.source_catalog_error() {
		errors.push(FileError {
			path: root.to_path_buf(),
			error: error.to_string(),
		});
		errors.sort_by(|a, b| a.path.cmp(&b.path));
	}
	Ok((reports, errors))
}

fn filtered_source_set(
	source_set: &environment::SourceFileSet,
	cfg: &check::Config,
) -> environment::SourceFileSet {
	let excludes = check::UriExclusionMatcher::new(&cfg.exclude.uris);
	filter_source_set(source_set, &excludes)
}

fn filter_source_set(
	source_set: &environment::SourceFileSet,
	excludes: &check::UriExclusionMatcher,
) -> environment::SourceFileSet {
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

#[derive(Clone, Copy)]
struct SourceSetCheckMode {
	report: bool,
	workspace_rules: bool,
}

fn check_source_set(
	source_set: &environment::SourceFileSet,
	cfg: &check::Config,
	scheme: &str,
	requirements: Option<&dyn check::RequirementResolver>,
	workspace: &dyn CheckWorkspace,
	mode: SourceSetCheckMode,
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
	let outcomes: Vec<Result<CheckedSourceFile, FileError>> = files
		.par_iter()
		.map(|f| {
			let f = *f;
			let rules = &compiled[&f.lang];
			let ctx = &source_set.roots[f.source].ctx;
			let check_ctx = CompiledCheck {
				scheme,
				compiled: rules,
				report: mode.report,
				workspace,
				requirements,
			};
			check_source_file_compiled(f, ctx, &check_ctx).map_err(|e| FileError {
				path: f.path.clone(),
				error: format!("{e:#}"),
			})
		})
		.collect();
	let mut checked = Vec::new();
	let mut errors = Vec::new();
	for o in outcomes {
		match o {
			Ok(r) => checked.push(r),
			Err(e) => errors.push(e),
		}
	}
	checked.sort_by(|a, b| a.report.path.cmp(&b.report.path));
	if mode.workspace_rules && errors.is_empty() {
		apply_workspace_rules(
			&mut checked,
			source_set,
			cfg,
			scheme,
			mode.report,
			workspace,
		)?;
	}
	let reports = checked.into_iter().map(|checked| checked.report).collect();
	errors.sort_by(|a, b| a.path.cmp(&b.path));
	Ok((reports, errors))
}

fn apply_workspace_rules(
	checked: &mut [CheckedSourceFile],
	source_set: &environment::SourceFileSet,
	cfg: &check::Config,
	scheme: &str,
	report: bool,
	workspace: &dyn CheckWorkspace,
) -> anyhow::Result<()> {
	let compiled = crate::check::workspace_eval::compile_workspace_rules(cfg, scheme)?;
	if compiled.is_empty() {
		return Ok(());
	}
	let (mut evaluation, source_map) = if compiled.has_linkage_rules() {
		let snapshot = workspace
			.linked_snapshot(source_set, scheme)?
			.ok_or_else(|| {
				anyhow::anyhow!(
					"workspace linkage rules were not run: this check workspace does not provide a linked snapshot"
				)
			})?;
		let source_map = linked_source_map(&snapshot, checked);
		let universe = linked_symbol_universe(&snapshot, &source_map);
		let evaluation = crate::check::workspace_eval::evaluate_workspace_rules_linked_in(
			&snapshot.index,
			&snapshot.linkage,
			&universe,
			&compiled,
			report,
		)?;
		(evaluation, Some(source_map))
	} else {
		let generation = ResourceGeneration::new(0);
		let sources = workspace_source_records(checked);
		let symbols = workspace_symbol_records(checked, scheme);
		let inventory = SymbolInventoryIndex::build(generation, &sources, &symbols);
		(
			crate::check::workspace_eval::evaluate_workspace_rules(&inventory, &compiled, report),
			None,
		)
	};
	if let Some(source_map) = &source_map {
		evaluation.violations.retain_mut(|violation| {
			let Some(checked_idx) = source_map.get(&violation.source.file()) else {
				return false;
			};
			violation.source = SourceId::at(*checked_idx);
			true
		});
	}
	let kept_by_rule = merge_workspace_violations(checked, evaluation.violations);
	for rule_report in &mut evaluation.reports {
		rule_report.violations = kept_by_rule
			.get(&rule_report.rule_id)
			.copied()
			.unwrap_or_default();
		if rule_report.verdict.is_some() {
			rule_report.verdict = Some(if rule_report.violations > 0 {
				crate::RuleVerdict::Fail
			} else if rule_report.inconclusive.unwrap_or_default() > 0 {
				crate::RuleVerdict::Inconclusive
			} else {
				crate::RuleVerdict::Pass
			});
		}
	}
	if report && let Some(first) = checked.first_mut() {
		first.report.rule_reports.extend(evaluation.reports);
		first
			.report
			.rule_reports
			.sort_by(|left, right| left.rule_id.cmp(&right.rule_id));
	}
	Ok(())
}

fn linked_source_map(
	snapshot: &WorkspaceSnapshot,
	checked: &[CheckedSourceFile],
) -> std::collections::HashMap<usize, usize> {
	use std::collections::HashMap;

	let by_path = checked
		.iter()
		.enumerate()
		.map(|(idx, checked)| (checked.file.path.clone(), idx))
		.collect::<HashMap<_, _>>();
	let by_root_relative = checked
		.iter()
		.enumerate()
		.map(|(idx, checked)| ((checked.file.source, checked.file.rel_path.clone()), idx))
		.collect::<HashMap<_, _>>();
	snapshot
		.index
		.sources
		.iter()
		.filter_map(|source| {
			let checked_idx = by_path
				.get(Path::new(&source.path))
				.or_else(|| {
					by_root_relative.get(&(source.source_root, PathBuf::from(&source.rel_path)))
				})
				.copied()?;
			Some((source.id.file(), checked_idx))
		})
		.collect()
}

fn linked_symbol_universe(
	snapshot: &WorkspaceSnapshot,
	source_map: &std::collections::HashMap<usize, usize>,
) -> SymbolSet {
	let mut universe = SymbolSet::new();
	for source_idx in source_map.keys() {
		if let Some(symbols) = snapshot
			.index
			.inventory
			.facets()
			.symbols_by_source(SourceId::at(*source_idx))
		{
			universe.union_with(symbols);
		}
	}
	universe
}

fn workspace_source_records(checked: &[CheckedSourceFile]) -> Vec<SourceFileRecord> {
	checked
		.iter()
		.enumerate()
		.map(|(file_idx, checked)| SourceFileRecord {
			id: SourceId::at(file_idx),
			uri: checked.file.path.display().to_string(),
			source_root: checked.file.source,
			path: checked.file.path.display().to_string(),
			rel_path: checked.file.rel_path.display().to_string(),
			anchor: checked.file.anchor.display().to_string(),
			language: checked.file.lang.tag().to_string(),
			text: String::new(),
		})
		.collect()
}

fn workspace_symbol_records(
	checked: &[CheckedSourceFile],
	scheme: &str,
) -> RecordTable<code_moniker_workspace::snapshot::SymbolRecord> {
	let shards = checked
		.iter()
		.enumerate()
		.map(|(file_idx, checked)| {
			Arc::from(environment::symbol_records_for_graph(
				file_idx,
				SourceId::at(file_idx),
				&checked.graph,
				&checked.source,
				checked.file.lang,
				scheme,
			))
		})
		.collect();
	RecordTable::from_shards(shards)
}

fn merge_workspace_violations(
	checked: &mut [CheckedSourceFile],
	violations: Vec<crate::check::workspace_eval::WorkspaceSymbolViolation>,
) -> BTreeMap<String, usize> {
	let mut by_source =
		BTreeMap::<usize, Vec<crate::check::workspace_eval::WorkspaceSymbolViolation>>::new();
	for workspace_violation in violations {
		by_source
			.entry(workspace_violation.source.file())
			.or_default()
			.push(workspace_violation);
	}
	let mut kept_by_rule = BTreeMap::new();
	for (file_idx, workspace_violations) in by_source {
		let Some(checked) = checked.get_mut(file_idx) else {
			continue;
		};
		let (suppressible, fixed): (
			Vec<crate::check::workspace_eval::WorkspaceSymbolViolation>,
			Vec<crate::check::workspace_eval::WorkspaceSymbolViolation>,
		) = workspace_violations
			.into_iter()
			.partition(|violation| violation.source_suppression);
		let mut suppressible = check::apply_suppressions(
			&checked.graph,
			&checked.source,
			suppressible
				.into_iter()
				.map(|violation| violation.violation)
				.collect(),
		);
		let mut violations = fixed
			.into_iter()
			.map(|violation| violation.violation)
			.collect::<Vec<_>>();
		violations.append(&mut suppressible);
		for violation in &violations {
			*kept_by_rule
				.entry(violation.rule_id.to_owned())
				.or_insert(0) += 1;
		}
		checked.report.violations.extend(violations);
		checked.report.violations.sort_by(|left, right| {
			left.lines
				.cmp(&right.lines)
				.then_with(|| left.rule_id.cmp(&right.rule_id))
		});
	}
	kept_by_rule
}

struct FileRequirementResolver<'a> {
	root: PathBuf,
	source_set: OnceLock<Result<environment::SourceFileSet, String>>,
	file_defs: OnceLock<Vec<OnceLock<Vec<DefRecord>>>>,
	excludes: check::UriExclusionMatcher,
	workspace: &'a dyn CheckWorkspace,
}

impl<'a> FileRequirementResolver<'a> {
	fn new(
		root: PathBuf,
		source_set: impl Into<Option<environment::SourceFileSet>>,
		exclude_uris: &[String],
		workspace: &'a dyn CheckWorkspace,
	) -> Self {
		let source_set_cell = OnceLock::new();
		if let Some(source_set) = source_set.into() {
			source_set_cell
				.set(Ok(source_set))
				.unwrap_or_else(|_| unreachable!("new source catalog cell"));
		}
		Self {
			root,
			source_set: source_set_cell,
			file_defs: OnceLock::new(),
			excludes: check::UriExclusionMatcher::new(exclude_uris),
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
		use rayon::prelude::*;

		let Ok(source_set) = self.source_set() else {
			return Vec::new();
		};
		let file_defs = self.file_defs(source_set.files.len());
		let candidate_indexes = source_set
			.files
			.iter()
			.enumerate()
			.filter_map(|(idx, file)| {
				file.root_moniker
					.as_ref()
					.is_some_and(|root| {
						root != &owner.moniker && owner.moniker.is_ancestor_of(root)
					})
					.then_some(idx)
			})
			.collect::<Vec<_>>();
		candidate_indexes.par_iter().for_each(|idx| {
			file_defs[*idx].get_or_init(|| {
				collect_file_defs(&source_set.files[*idx], source_set, self.workspace)
			});
		});
		candidate_indexes
			.into_iter()
			.flat_map(|idx| {
				file_defs[idx]
					.get()
					.into_iter()
					.flat_map(|defs| defs.iter())
			})
			.filter(|def| owner.moniker.is_ancestor_of(&def.moniker))
			.filter(|def| lazy_domain_matches(inner, def))
			.collect()
	}
}

impl FileRequirementResolver<'_> {
	fn file_defs(&self, file_count: usize) -> &[OnceLock<Vec<DefRecord>>] {
		self.file_defs
			.get_or_init(|| (0..file_count).map(|_| OnceLock::new()).collect())
			.as_slice()
	}

	fn source_set(&self) -> Result<&environment::SourceFileSet, &str> {
		match self.source_set.get_or_init(|| {
			self.workspace
				.source_catalog(&self.root)
				.map(|source_set| filter_source_set(&source_set, &self.excludes))
				.map_err(|error| {
					format!(
						"cannot build lazy source catalog for `{}`: {error:#}",
						self.root.display()
					)
				})
		}) {
			Ok(source_set) => Ok(source_set),
			Err(error) => Err(error),
		}
	}

	fn source_catalog_error(&self) -> Option<&str> {
		self.source_set
			.get()
			.and_then(|result| result.as_ref().err())
			.map(String::as_str)
	}
}

fn collect_file_defs(
	file: &environment::SourceFile,
	source_set: &environment::SourceFileSet,
	workspace: &dyn CheckWorkspace,
) -> Vec<DefRecord> {
	let Ok(source) = workspace.read_to_string(&file.path) else {
		return Vec::new();
	};
	let ctx = &source_set.roots[file.source].ctx;
	let graph = environment::extract_source_with(file.lang, &source, &file.anchor, ctx);
	if file
		.root_moniker
		.as_ref()
		.is_none_or(|catalog_root| catalog_root != graph.root())
	{
		return Vec::new();
	}
	graph.defs().cloned().collect()
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
		Domain::Pairs(_)
		| Domain::Segments
		| Domain::OutRefs
		| Domain::InRefs
		| Domain::SourceOutRefs
		| Domain::SourceInRefs
		| Domain::TargetOutRefs
		| Domain::TargetInRefs
		| Domain::SourceAncestorOutRefs
		| Domain::SourceAncestorInRefs => false,
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

fn violations_by_srcset(reports: &[FileReport]) -> std::collections::BTreeMap<String, usize> {
	let mut counts = std::collections::BTreeMap::new();
	let mut unspecified = 0usize;
	for report in reports {
		for violation in &report.violations {
			if let Some(srcset) = &violation.srcset {
				*counts.entry(srcset.clone()).or_default() += 1;
			} else {
				unspecified += 1;
			}
		}
	}
	if !counts.is_empty() && unspecified > 0 {
		counts.insert("unspecified".to_string(), unspecified);
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

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Mutex;
	use std::sync::atomic::{AtomicUsize, Ordering};

	struct RecordingWorkspace {
		inner: MemoryCheckWorkspace,
		reads: Mutex<Vec<PathBuf>>,
		catalog_calls: AtomicUsize,
		fail_catalog: bool,
	}

	impl RecordingWorkspace {
		fn new(inner: MemoryCheckWorkspace) -> Self {
			Self {
				inner,
				reads: Mutex::new(Vec::new()),
				catalog_calls: AtomicUsize::new(0),
				fail_catalog: false,
			}
		}

		fn with_catalog_error(mut self) -> Self {
			self.fail_catalog = true;
			self
		}

		fn reads(&self) -> Vec<PathBuf> {
			self.reads.lock().expect("read log").clone()
		}
	}

	impl CheckWorkspace for RecordingWorkspace {
		fn is_dir(&self, path: &Path) -> anyhow::Result<bool> {
			self.inner.is_dir(path)
		}

		fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
			self.reads
				.lock()
				.expect("read log")
				.push(path.to_path_buf());
			self.inner.read_to_string(path)
		}

		fn source_set(
			&self,
			root: &Path,
			files: &[PathBuf],
		) -> anyhow::Result<environment::SourceFileSet> {
			self.inner.source_set(root, files)
		}

		fn source_catalog(&self, root: &Path) -> anyhow::Result<environment::SourceFileSet> {
			self.catalog_calls.fetch_add(1, Ordering::Relaxed);
			if self.fail_catalog {
				anyhow::bail!("catalog unavailable");
			}
			self.inner.source_catalog(root)
		}

		fn exists(&self, path: &Path) -> bool {
			self.inner.exists(path)
		}

		fn linked_snapshot(
			&self,
			source_set: &environment::SourceFileSet,
			scheme: &str,
		) -> anyhow::Result<Option<Arc<WorkspaceSnapshot>>> {
			self.inner.linked_snapshot(source_set, scheme)
		}
	}

	#[test]
	fn check_request_rejects_source_groups_from_another_project_rules_file() {
		let analyzed = tempfile::tempdir().expect("analyzed project");
		let external = tempfile::tempdir().expect("external rules project");
		let external_rules = external.path().join(".code-moniker.toml");
		std::fs::write(
			&external_rules,
			r#"
default_rules = false

[[workspace.source_group]]
roots = ["src"]
"#,
		)
		.expect("write external project config");
		let request = CheckRequest::new(
			analyzed.path(),
			RuleSetRequest::with_rules(&external_rules, "code+moniker://"),
		);

		let error = request
			.run()
			.expect_err("structural config must belong to the analyzed project");
		assert!(
			error
				.to_string()
				.contains("may be declared only in the canonical"),
			"{error:#}"
		);
	}

	#[test]
	fn descendant_catalog_reads_only_candidate_roots_once() {
		let root = Path::new("/project");
		let workspace = RecordingWorkspace::new(
			MemoryCheckWorkspace::new(root)
				.with_file("src/tools/mod.rs", "mod read; mod symbols;", Lang::Rs)
				.with_file("src/tools/read.rs", "fn same_helper() {}", Lang::Rs)
				.with_file("src/tools/symbols.rs", "fn same_helper() {}", Lang::Rs)
				.with_file("src/unrelated.rs", "fn same_helper() {}", Lang::Rs),
		);
		let catalog = workspace.source_catalog(root).expect("source catalog");
		let resolver = FileRequirementResolver::new(root.to_path_buf(), catalog, &[], &workspace);
		let graph = environment::extract_source_with(
			Lang::Rs,
			"mod read; mod symbols;",
			Path::new("src/tools/mod.rs"),
			&environment::ExtractContext::default(),
		);
		let owner = graph.defs().next().expect("module root");
		let domain = Domain::Children("fn".to_string());

		let first = check::RequirementResolver::descendant_defs(&resolver, owner, &domain);
		assert_eq!(first.len(), 2);
		let second = check::RequirementResolver::descendant_defs(&resolver, owner, &domain);
		assert_eq!(second.len(), 2);

		let mut reads = workspace.reads();
		reads.sort();
		assert_eq!(
			reads,
			vec![
				root.join("src/tools/read.rs"),
				root.join("src/tools/symbols.rs")
			]
		);
		assert!(!reads.contains(&root.join("src/unrelated.rs")));
		assert!(!reads.contains(&root.join("src/tools/mod.rs")));
	}

	#[test]
	fn false_lazy_rule_does_not_build_the_source_catalog() {
		let root = Path::new("/project");
		let workspace = RecordingWorkspace::new(
			MemoryCheckWorkspace::new(root)
				.with_file("src/lib.rs", "pub fn ready() {}", Lang::Rs)
				.with_file("src/tools/mod.rs", "mod read;", Lang::Rs)
				.with_file("src/tools/read.rs", "fn helper() {}", Lang::Rs),
		);
		let rules = RuleSetRequest::new(None, "code+moniker://")
			.with_default_rules(DefaultRulesSelection::Disabled)
			.with_inline_rules(vec![
				r#"
				[[rust.module.where]]
				id = "tools-descendants"
				expr = "uri ~ '**/dir:src/module:tools' => count(descendants(fn)) = 0"
				message = "tools descendants"
				"#
				.to_string(),
			]);
		let request = CheckRequest::new(root, rules).with_files(vec![PathBuf::from("src/lib.rs")]);

		let run = request
			.run_with_workspace(&workspace)
			.expect("filtered check");

		assert_eq!(run.reports.len(), 1);
		assert!(run.reports[0].violations.is_empty());
		assert_eq!(workspace.catalog_calls.load(Ordering::Relaxed), 0);
		assert_eq!(workspace.reads(), vec![root.join("src/lib.rs")]);
	}

	#[test]
	fn reached_lazy_rule_reports_source_catalog_errors() {
		let root = Path::new("/project");
		let workspace = RecordingWorkspace::new(MemoryCheckWorkspace::new(root).with_file(
			"src/tools/mod.rs",
			"pub fn ready() {}",
			Lang::Rs,
		))
		.with_catalog_error();
		let rules = RuleSetRequest::new(None, "code+moniker://")
			.with_default_rules(DefaultRulesSelection::Disabled)
			.with_inline_rules(vec![
				r#"
				[[rust.module.where]]
				id = "tools-descendants"
				expr = "uri ~ '**/dir:src/module:tools' => count(descendants(fn)) = 0"
				message = "tools descendants"
				"#
				.to_string(),
			]);
		let request =
			CheckRequest::new(root, rules).with_files(vec![PathBuf::from("src/tools/mod.rs")]);

		let run = request
			.run_with_workspace(&workspace)
			.expect("filtered check");

		assert!(run.any_error());
		assert_eq!(run.errors.len(), 1);
		assert_eq!(run.errors[0].path, root);
		assert!(run.errors[0].error.contains("catalog unavailable"));
		assert_eq!(workspace.catalog_calls.load(Ordering::Relaxed), 1);
	}

	#[test]
	fn indexed_workspace_is_built_from_one_cached_snapshot_generation() {
		let temp = tempfile::tempdir().expect("tempdir");
		let source = temp.path().join("lib.rs");
		std::fs::write(&source, "pub fn stale() {}\n").expect("write initial source");
		let cache = LocalResourceCache::default();
		let mut registry = LocalWorkspaceRegistry::local_with_cache(
			LocalWorkspaceOptions::new(vec![temp.path().to_path_buf()], None),
			cache.clone(),
		);

		assert!(matches!(
			registry
				.commands()
				.refresh(WorkspaceRequest::new("initial")),
			WorkspaceTransition::Ready { .. }
		));
		let stale = registry.queries().snapshot_arc().expect("stale snapshot");
		let stale_material = cache
			.index_material(stale.index.generation)
			.expect("stale material");

		std::fs::write(&source, "pub fn current() {}\n").expect("write refreshed source");
		assert!(matches!(
			registry
				.commands()
				.refresh(WorkspaceRequest::new("refresh")),
			WorkspaceTransition::Ready { .. }
		));
		let current = registry.queries().snapshot_arc().expect("current snapshot");

		assert!(
			Arc::strong_count(&stale_material) > 0,
			"the old material remains available to a caller holding it"
		);
		let error = IndexedCheckWorkspace::from_snapshot(temp.path(), &cache, Arc::clone(&stale))
			.err()
			.expect("an evicted snapshot generation must be rejected");
		assert!(
			error
				.to_string()
				.contains(&stale.index.generation.value().to_string())
		);

		let workspace =
			IndexedCheckWorkspace::from_snapshot(temp.path(), &cache, Arc::clone(&current))
				.expect("current generation");
		assert_eq!(
			workspace.read_to_string(&source).expect("indexed source"),
			"pub fn current() {}\n"
		);
		assert_eq!(
			workspace
				.linked_snapshot(workspace.material.source_set(), "code+moniker://")
				.expect("linked snapshot")
				.expect("snapshot")
				.index
				.generation,
			current.index.generation
		);
	}
}
