use std::cmp::Ordering;
use std::path::Path;

use super::Scenario;
use super::expect::ExpectedViolation;
use crate::check::command::{CheckRequest, CheckRun, DefaultRulesSelection, RuleSetRequest};

const RULES_FILE: &str = ".code-moniker.toml";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScenarioRun {
	pub actual: Vec<ExpectedViolation>,
	pub missing: Vec<ExpectedViolation>,
	pub unexpected: Vec<ExpectedViolation>,
	pub errors: Vec<String>,
	pub silent_rules: Vec<String>,
}

impl ScenarioRun {
	fn from_check(run: &CheckRun, root: &Path, expects: &[ExpectedViolation]) -> Self {
		let actual = collect_actual(run, root);
		let (missing, unexpected) = diff_expectations(expects, &actual);
		Self {
			actual,
			missing,
			unexpected,
			errors: collect_errors(run, root),
			silent_rules: collect_silent_rules(run),
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
	pub fn materialize(&self, root: &Path) -> std::io::Result<()> {
		for file in &self.files {
			let path = root.join(&file.path);
			if let Some(parent) = path.parent() {
				std::fs::create_dir_all(parent)?;
			}
			std::fs::write(path, &file.body)?;
		}
		if let Some(rules) = &self.rules {
			std::fs::write(root.join(RULES_FILE), rules)?;
		}
		Ok(())
	}

	pub fn run(&self, root: &Path, scheme: &str) -> anyhow::Result<ScenarioRun> {
		let run = self.check_request(root, scheme).run()?;
		Ok(ScenarioRun::from_check(&run, root, &self.expects))
	}

	fn check_request(&self, root: &Path, scheme: &str) -> CheckRequest {
		CheckRequest::new(
			root.to_path_buf(),
			RuleSetRequest::with_rules(root.join(RULES_FILE), scheme).with_default_rules(
				DefaultRulesSelection::from_override(Some(self.effective_default_rules())),
			),
		)
		.with_report(true)
	}

	pub fn bless(&self, document: &str, actual: &[ExpectedViolation]) -> String {
		let mut body = actual
			.iter()
			.map(ToString::to_string)
			.collect::<Vec<_>>()
			.join("\n");
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

fn collect_silent_rules(run: &CheckRun) -> Vec<String> {
	run.rule_violation_totals()
		.into_iter()
		.filter_map(|(rule_id, violations)| (violations == 0).then(|| rule_id.to_string()))
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
