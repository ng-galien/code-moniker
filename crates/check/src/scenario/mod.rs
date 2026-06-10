//! Executable check scenarios: a Markdown document describing a file layout,
//! an inline rules overlay, and the violations the layout is expected to
//! produce. One document materializes into a workspace the scan pipeline can
//! run against; see `docs/check-scenarios.md` for the format contract.

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
	pub lang: String,
	pub blurb: String,
	pub published: bool,
	pub default_rules: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioFile {
	pub path: String,
	pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scenario {
	pub meta: ScenarioMeta,
	pub rules: Option<String>,
	pub files: Vec<ScenarioFile>,
	pub expects: Vec<ExpectedViolation>,
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
