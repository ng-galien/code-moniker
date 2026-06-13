use std::cmp::Ordering;
use std::path::Path;

use super::Scenario;
use super::expect::ExpectedViolation;
use crate::check::command::{
	CheckRun, FileError, FileReport, MemoryCheckWorkspace, check_project_files_workspace,
	check_project_workspace,
};
use crate::check::config;
use code_moniker_core::lang::Lang;
use code_moniker_workspace::lang::path_to_lang;

const RULES_FILE: &str = ".code-moniker.toml";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScenarioRun {
	pub actual: Vec<ExpectedViolation>,
	pub missing: Vec<ExpectedViolation>,
	pub unexpected: Vec<ExpectedViolation>,
	pub errors: Vec<String>,
	pub silent_rules: Vec<String>,
	pub stale_undemonstrated: Vec<String>,
}

impl ScenarioRun {
	fn from_check(run: &CheckRun, root: &Path, scenario: &Scenario) -> Self {
		let actual = collect_actual(run, root);
		let (missing, unexpected) = diff_expectations(&scenario.expects, &actual);
		let (silent_rules, stale_undemonstrated) = coverage(run, scenario);
		Self {
			actual,
			missing,
			unexpected,
			errors: collect_errors(run, root),
			silent_rules,
			stale_undemonstrated,
		}
	}

	pub fn is_match(&self) -> bool {
		self.missing.is_empty() && self.unexpected.is_empty() && self.errors.is_empty()
	}

	pub fn mismatch_summary(&self) -> String {
		let mut lines = Vec::new();
		for missing in &self.missing {
			lines.push(format!("missing:    {missing}"));
		}
		for unexpected in &self.unexpected {
			lines.push(format!("unexpected: {unexpected}"));
		}
		for error in &self.errors {
			lines.push(format!("error:      {error}"));
		}
		lines.join("\n")
	}
}

impl Scenario {
	pub fn run(&self, root: &Path, scheme: &str) -> anyhow::Result<ScenarioRun> {
		let run = self.check(root, &[], scheme, true)?;
		Ok(ScenarioRun::from_check(&run, root, self))
	}

	pub fn check(
		&self,
		root: &Path,
		files: &[std::path::PathBuf],
		scheme: &str,
		report: bool,
	) -> anyhow::Result<CheckRun> {
		let cfg = config::load_from_str(
			self.rules.as_deref().unwrap_or(""),
			RULES_FILE,
			Some(self.effective_default_rules()),
		)?;
		let workspace = self.memory_workspace(root)?;
		let (reports, errors) = check_scenario_workspace(&workspace, files, &cfg, scheme, report)?;
		Ok(CheckRun {
			reports,
			errors,
			elapsed_ms: 0,
			skip_reason: None,
		})
	}

	fn memory_workspace(&self, root: &Path) -> anyhow::Result<MemoryCheckWorkspace> {
		let mut workspace = MemoryCheckWorkspace::new(root);
		for file in &self.files {
			let Some(lang) = scenario_file_lang(file)? else {
				continue;
			};
			workspace = workspace.with_file(Path::new(&file.path), &file.body, lang);
		}
		Ok(workspace)
	}

	pub fn bless(&self, document: &str, actual: &[ExpectedViolation]) -> String {
		let mut entries: Vec<String> = self
			.undemonstrated
			.iter()
			.map(|rule| format!("! {} {}", rule.rule_id, rule.reason))
			.collect();
		entries.extend(actual.iter().map(ToString::to_string));
		let mut body = entries.join("\n");
		if !body.is_empty() {
			body.push('\n');
		}
		match self.expect_span {
			Some((start, end)) => format!("{}{}{}", &document[..start], body, &document[end..]),
			None => {
				let separator = if document.ends_with('\n') { "" } else { "\n" };
				format!("{document}{separator}\n```cm:expect\n{body}```\n")
			}
		}
	}
}

fn check_scenario_workspace(
	workspace: &MemoryCheckWorkspace,
	files: &[std::path::PathBuf],
	cfg: &crate::check::Config,
	scheme: &str,
	report: bool,
) -> anyhow::Result<(Vec<FileReport>, Vec<FileError>)> {
	if files.is_empty() {
		return check_project_workspace(workspace.root(), cfg, scheme, report, workspace);
	}
	check_project_files_workspace(workspace.root(), files, cfg, scheme, report, workspace)
}

fn scenario_file_lang(file: &super::ScenarioFile) -> anyhow::Result<Option<Lang>> {
	match file.fence.as_str() {
		"" => Ok(Some(path_to_lang(Path::new(&file.path))?)),
		"rust" | "rs" => Ok(Some(Lang::Rs)),
		"ts" | "typescript" => Ok(Some(Lang::Ts)),
		"python" | "py" => Ok(Some(Lang::Python)),
		"go" => Ok(Some(Lang::Go)),
		"java" => Ok(Some(Lang::Java)),
		"cs" | "csharp" => Ok(Some(Lang::Cs)),
		"sql" | "plpgsql" => Ok(Some(Lang::Sql)),
		"text" | "txt" | "md" | "markdown" => Ok(None),
		_ => Ok(Some(path_to_lang(Path::new(&file.path))?)),
	}
}

fn collect_actual(run: &CheckRun, root: &Path) -> Vec<ExpectedViolation> {
	let mut actual: Vec<_> = run
		.file_violations()
		.map(|(path, violation)| ExpectedViolation {
			rule_id: violation.rule_id.clone(),
			path: relative_display(path, root),
			lines: violation.lines,
		})
		.collect();
	actual.sort();
	actual
}

fn collect_errors(run: &CheckRun, root: &Path) -> Vec<String> {
	run.error_summaries()
		.map(|(path, error)| format!("{}: {error}", relative_display(path, root)))
		.collect()
}

fn coverage(run: &CheckRun, scenario: &Scenario) -> (Vec<String>, Vec<String>) {
	let excused: Vec<&str> = scenario
		.undemonstrated
		.iter()
		.map(|rule| rule.rule_id.as_str())
		.collect();
	let silent = collect_silent_rules(run);
	let stale = excused
		.iter()
		.filter(|rule_id| !silent.iter().any(|silent| silent == *rule_id))
		.map(ToString::to_string)
		.collect();
	let silent = silent
		.into_iter()
		.filter(|rule_id| !excused.contains(&rule_id.as_str()))
		.collect();
	(silent, stale)
}

fn collect_silent_rules(run: &CheckRun) -> Vec<String> {
	run.rule_violation_totals()
		.into_iter()
		.filter(|(_, violations)| *violations == 0)
		.map(|(rule_id, _)| rule_id.to_string())
		.collect()
}

fn relative_display(path: &Path, root: &Path) -> String {
	path.strip_prefix(root)
		.unwrap_or(path)
		.display()
		.to_string()
		.replace('\\', "/")
}

fn diff_expectations(
	expected: &[ExpectedViolation],
	actual: &[ExpectedViolation],
) -> (Vec<ExpectedViolation>, Vec<ExpectedViolation>) {
	let mut missing = Vec::new();
	let mut unexpected = Vec::new();
	let mut left = expected.iter().peekable();
	let mut right = actual.iter().peekable();
	while let (Some(expected), Some(actual)) = (left.peek(), right.peek()) {
		match expected.cmp(actual) {
			Ordering::Equal => {
				left.next();
				right.next();
			}
			Ordering::Less => missing.extend(left.next().cloned()),
			Ordering::Greater => unexpected.extend(right.next().cloned()),
		}
	}
	missing.extend(left.cloned());
	unexpected.extend(right.cloned());
	(missing, unexpected)
}
