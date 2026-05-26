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
pub(crate) mod moniker_render;
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
