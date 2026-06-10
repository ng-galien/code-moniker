//! Standalone CLI surface. See `docs/cli/extract.md` (per-file probe)
//! and `docs/cli/check.md` (workspace linter).

pub(crate) mod args;
pub(crate) mod check;
pub(crate) mod color;
pub(crate) mod extract;
pub(crate) mod harness;
pub(crate) mod langs;
pub(crate) mod language_kinds;
#[cfg(feature = "mcp")]
pub(crate) mod live_control;
pub(crate) mod manifest;
#[cfg(feature = "mcp")]
pub(crate) mod mcp;
#[cfg(feature = "mcp")]
pub(crate) mod mcp_command;
pub(crate) mod page;
pub(crate) mod rules;
#[cfg(any(feature = "tui", feature = "mcp"))]
pub(crate) mod session;
pub(crate) mod shapes;
pub(crate) mod stats;
#[cfg(feature = "pretty")]
pub(crate) mod tree;
#[cfg(feature = "tui")]
pub(crate) mod ui;
#[cfg(feature = "tui")]
pub(crate) mod ui_command;
#[cfg(any(feature = "tui", feature = "mcp"))]
pub(crate) mod views;
#[cfg(any(feature = "tui", feature = "mcp"))]
pub(crate) mod workspace_index;

use std::io::Write;
use std::process::ExitCode;

#[cfg(any(feature = "tui", feature = "mcp"))]
pub use args::LiveRefresh;
#[cfg(feature = "mcp")]
pub use args::McpArgs;
#[cfg(feature = "tui")]
pub use args::UiArgs;
pub use args::{
	Charset, CheckArgs, CheckFormat, Cli, CodexHarnessArgs, ColorChoice, Command, DefaultRules,
	ExtractArgs, HarnessArgs, HarnessCommand, HarnessToolBackend, HarnessToolFilesArgs, LangsArgs,
	LangsFormat, ManifestArgs, ManifestFormat, MonikerFormat, OutputFormat, OutputMode, RulesArgs,
	RulesCommand, RulesFileArgs, RulesLearnArgs, RulesLearnFormat, RulesShowArgs, RulesShowFormat,
	ShapesArgs, StatsArgs, StatsFormat,
};
pub use code_moniker_workspace::lang::{LangError, path_to_lang};
pub use extract::{MatchSet, Predicate, RefMatch};

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
		Command::Ui(args) => ui_command::run(args, stdout, stderr),
		#[cfg(feature = "mcp")]
		Command::Mcp(args) => mcp_command::run(args, stdout, stderr),
		Command::Langs(args) => langs::run(args, stdout, stderr),
		Command::Shapes(args) => shapes::run(args, stdout, stderr),
		Command::Manifest(args) => manifest::run(args, stdout, stderr),
	}
}
