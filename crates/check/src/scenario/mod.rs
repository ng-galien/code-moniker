//! Executable check scenarios: a Markdown document describing a file layout,
//! an inline rules overlay, and the violations the layout is expected to
//! produce. One document feeds an in-memory workspace the scan pipeline can run
//! against; see `docs/check-scenarios.md` for the format contract.

use crate::RuleVerdict;

mod expect;
mod parse;
mod run;
#[cfg(test)]
mod tests;

pub use expect::ExpectedViolation;
pub use parse::ScenarioError;
pub use run::ScenarioRun;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScenarioMeta {
	pub name: String,
	pub title: String,
	pub lang: String,
	pub blurb: String,
	pub summary: String,
	pub learn_kind: String,
	pub learn_path: String,
	pub learn_order: Option<u32>,
	pub tags: Vec<String>,
	pub learn_aliases: Vec<String>,
	pub published: bool,
	pub default_rules: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioFile {
	pub path: String,
	pub fence: String,
	pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndemonstratedRule {
	pub rule_id: String,
	pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedRuleVerdict {
	pub rule_id: String,
	pub verdict: RuleVerdict,
}

impl std::fmt::Display for ExpectedRuleVerdict {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"verdict {} = {}",
			self.rule_id,
			rule_verdict_name(self.verdict)
		)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleVerdictMismatch {
	pub rule_id: String,
	pub expected: RuleVerdict,
	pub actual: Option<RuleVerdict>,
}

impl std::fmt::Display for RuleVerdictMismatch {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"verdict:   {} expected {}, actual {}",
			self.rule_id,
			rule_verdict_name(self.expected),
			self.actual.map(rule_verdict_name).unwrap_or("absent")
		)
	}
}

fn rule_verdict_name(verdict: RuleVerdict) -> &'static str {
	match verdict {
		RuleVerdict::Pass => "pass",
		RuleVerdict::Fail => "fail",
		RuleVerdict::Inconclusive => "inconclusive",
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scenario {
	pub meta: ScenarioMeta,
	pub rules: Option<String>,
	pub files: Vec<ScenarioFile>,
	pub expects: Vec<ExpectedViolation>,
	pub verdicts: Vec<ExpectedRuleVerdict>,
	pub undemonstrated: Vec<UndemonstratedRule>,
	pub(crate) expect_span: Option<(usize, usize)>,
}

impl Scenario {
	pub fn parse(document: &str) -> Result<Self, ScenarioError> {
		parse::parse_document(document)
	}

	pub fn effective_default_rules(&self) -> bool {
		self.meta.default_rules.unwrap_or(self.rules.is_none())
	}
}
