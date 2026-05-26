//! Standalone CLI surface. See `docs/cli/extract.md` (per-file probe)
//! and `docs/cli/check.md` (project linter).

pub mod args;
pub mod check;
pub(crate) mod color;
pub mod extract;
pub mod format;
pub mod harness;
pub mod langs;
pub(crate) mod language_kinds;
pub mod manifest;
pub(crate) mod page;
#[cfg(feature = "tui")]
pub(crate) mod perf;
pub mod rules;
pub mod session;
pub mod shapes;
pub mod stats;
#[cfg(feature = "pretty")]
pub(crate) mod tree;
#[cfg(feature = "tui")]
pub mod ui;

use std::io::Write;
use std::process::ExitCode;

#[cfg(feature = "tui")]
pub use args::UiArgs;
pub use args::{
	CheckArgs, CheckFormat, Cli, CodexHarnessArgs, Command, DefaultRules, ExtractArgs, HarnessArgs,
	HarnessCommand, HarnessToolBackend, HarnessToolFilesArgs, LangsArgs, LangsFormat, ManifestArgs,
	ManifestFormat, OutputFormat, OutputMode, RulesArgs, RulesCommand, RulesFileArgs,
	RulesLearnArgs, RulesLearnFormat, RulesShowArgs, RulesShowFormat, ShapesArgs, StatsArgs,
	StatsFormat,
};
pub use code_moniker_workspace::lang::{LangError, path_to_lang};
pub use extract::{MatchSet, Predicate};

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
		Command::Extract(args) => extract::run(args, stdout, stderr),
		Command::Stats(args) => stats::run(args, stdout, stderr),
		Command::Check(args) => check::run(args, stdout, stderr),
		Command::Rules(args) => rules::run(args, stdout, stderr),
		Command::Harness(args) => harness::run(args, stdout, stderr),
		#[cfg(feature = "tui")]
		Command::Ui(args) => ui::run(args, stdout, stderr),
		Command::Langs(args) => langs::run(args, stdout, stderr),
		Command::Shapes(args) => shapes::run(args, stdout, stderr),
		Command::Manifest(args) => manifest::run(args, stdout, stderr),
	}
}
