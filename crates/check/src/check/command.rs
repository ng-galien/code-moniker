use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use code_moniker_core::core::code_graph::DefRecord;
use code_moniker_workspace::environment;
use code_moniker_workspace::lang::path_to_lang;

use crate::check;
use crate::check::expr::Domain;

/// One scanned file's rule outcome: the suppression-filtered violations and,
/// when `report` is requested, the per-rule observability counts.
pub struct FileReport {
	pub path: PathBuf,
	pub violations: Vec<check::Violation>,
	pub rule_reports: Vec<check::RuleReport>,
}

/// A per-file I/O or extraction failure, accumulated rather than aborting a
/// project scan.
pub struct FileError {
	pub path: PathBuf,
	pub error: String,
}

pub fn check_one_file(
	path: &Path,
	cfg: &check::Config,
	scheme: &str,
	report: bool,
) -> anyhow::Result<Option<FileReport>> {
	let Ok(lang) = path_to_lang(path) else {
		return Ok(None);
	};
	let excludes = check::UriExclusionMatcher::new(&cfg.exclude.uris);
	if excludes.matches_path(path) {
		return Ok(None);
	}
	let compiled = check::compile_rules(cfg, lang, scheme)?;
	check_one_compiled(path, None, lang, scheme, &compiled, report).map(Some)
}

/// `moniker_anchor` overrides the path passed to the extractor - used by
/// project mode to anchor each file's moniker on its path relative to the
/// scan root. `None` means "same as `fs_path`" (single-file mode).
fn check_one_compiled(
	fs_path: &Path,
	moniker_anchor: Option<&Path>,
	lang: code_moniker_core::lang::Lang,
	scheme: &str,
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
	let raw = check::evaluate_compiled(&graph, &source, lang, scheme, compiled);
	let violations = check::apply_suppressions(&graph, &source, raw);
	let rule_reports = if report {
		let mut rule_reports = check::rule_report_compiled(&graph, &source, lang, scheme, compiled);
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
	scheme: &str,
	compiled: &check::CompiledRules,
	report: bool,
	requirements: Option<&dyn check::RequirementResolver>,
) -> anyhow::Result<FileReport> {
	let source = std::fs::read_to_string(&file.path)
		.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", file.path.display()))?;
	let graph = environment::extract_source_with(file.lang, &source, &file.anchor, ctx);
	let raw = check::evaluate_compiled_with_requirements(
		&graph,
		&source,
		file.lang,
		scheme,
		compiled,
		requirements,
	);
	let violations = check::apply_suppressions(&graph, &source, raw);
	let rule_reports = if report {
		let mut rule_reports = check::rule_report_compiled_with_requirements(
			&graph,
			&source,
			file.lang,
			scheme,
			compiled,
			requirements,
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
	let source_set = environment::discover_sources(&[root.to_path_buf()], None)?;
	let requirements = FileRequirementResolver::new(
		root.to_path_buf(),
		Some(filtered_source_set(&source_set, cfg)),
	);
	check_source_set(&source_set, cfg, scheme, report, Some(&requirements))
}

pub fn check_project_files(
	root: &Path,
	files: &[PathBuf],
	cfg: &check::Config,
	scheme: &str,
	report: bool,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	let source_set = environment::discover_source_files(root, files, None)?;
	let resolver_source_set = environment::discover_sources(&[root.to_path_buf()], None)?;
	let requirements = FileRequirementResolver::new(
		root.to_path_buf(),
		Some(filtered_source_set(&resolver_source_set, cfg)),
	);
	check_source_set(&source_set, cfg, scheme, report, Some(&requirements))
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
			check_source_file_compiled(f, ctx, scheme, rules, report, requirements).map_err(|e| {
				FileError {
					path: f.path.clone(),
					error: format!("{e:#}"),
				}
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

struct FileRequirementResolver {
	root: PathBuf,
	source_set: Option<environment::SourceFileSet>,
	workspace_defs: OnceLock<Vec<DefRecord>>,
}

impl FileRequirementResolver {
	fn new(root: PathBuf, source_set: Option<environment::SourceFileSet>) -> Self {
		Self {
			root,
			source_set,
			workspace_defs: OnceLock::new(),
		}
	}

	fn workspace_defs(&self) -> &[DefRecord] {
		self.workspace_defs
			.get_or_init(|| collect_workspace_defs(self.source_set.as_ref()))
	}
}

impl check::RequirementResolver for FileRequirementResolver {
	fn exists(&self, pattern: &str, _source: &DefRecord, _scheme: &str) -> bool {
		let Some(candidates) = source_candidates_from_requirement(&self.root, pattern) else {
			return false;
		};
		let Ok(path_pattern) = check::path::parse(pattern) else {
			return false;
		};
		for path in candidates {
			if !path.exists() {
				continue;
			}
			let Ok(lang) = path_to_lang(&path) else {
				continue;
			};
			let Ok(source) = std::fs::read_to_string(&path) else {
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

fn collect_workspace_defs(source_set: Option<&environment::SourceFileSet>) -> Vec<DefRecord> {
	let Some(source_set) = source_set else {
		return Vec::new();
	};
	let mut defs = Vec::new();
	for file in &source_set.files {
		let Ok(source) = std::fs::read_to_string(&file.path) else {
			continue;
		};
		let ctx = &source_set.roots[file.source].ctx;
		let graph = environment::extract_source_with(file.lang, &source, &file.anchor, ctx);
		defs.extend(graph.defs().cloned());
	}
	defs
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
