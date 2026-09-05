//! Standalone CLI surface. See `docs/cli/extract.md` (per-file probe)
//! and `docs/cli/check.md` (workspace linter).

pub(crate) mod agent;
pub(crate) mod args;
pub(crate) mod check;
pub(crate) mod check_scenario;
pub(crate) mod color;
pub(crate) mod daemon;
pub(crate) mod diff;
pub(crate) mod docs;
pub(crate) mod extract;
pub(crate) mod fs_nofollow;
pub(crate) mod git_runtime_supervisor;
pub(crate) mod hooks;
pub(crate) mod langs;
pub(crate) mod language_kinds;
pub(crate) mod manifest;
#[cfg(feature = "mcp")]
pub(crate) mod mcp;
#[cfg(feature = "mcp")]
pub(crate) mod mcp_command;
pub(crate) mod page;
pub(crate) mod predicate;
pub(crate) mod presentation;
pub(crate) mod query;
pub(crate) mod rules;
#[cfg(feature = "mcp")]
pub(crate) mod session;
pub(crate) mod shapes;
pub(crate) mod stats;
#[cfg(feature = "pretty")]
pub(crate) mod tree;
#[cfg(feature = "mcp")]
pub(crate) mod views;

use std::io::Write;
use std::process::ExitCode;

#[cfg(feature = "mcp")]
pub use args::McpArgs;
pub use args::{
	AgentArgs, AgentClient, AgentCommand, AgentComponent, AgentInspectArgs, AgentInstallArgs,
	AgentUninstallArgs, Charset, CheckArgs, CheckFormat, Cli, ColorChoice, Command, DaemonArgs,
	DaemonCommand, DaemonRootArgs, DaemonStartArgs, DaemonTargetArgs, DefaultRules, DiffArgs,
	DocsArgs, ExtractArgs, GitRuntimeArgs, HookInstallArgs, LangsArgs, LangsFormat, LiveRefresh,
	ManifestArgs, ManifestFormat, MonikerFormat, OutputFormat, OutputMode, QueryArgs, RulesArgs,
	RulesCommand, RulesEvalArgs, RulesFileArgs, RulesLearnArgs, RulesLearnFormat, RulesShowArgs,
	RulesShowFormat, ShapesArgs, StatsArgs, StatsFormat, ToolBackend, ToolFilesArgs,
};
pub use code_moniker_workspace::lang::{LangError, path_to_lang};
pub use extract::{MatchSet, RefMatch};
pub use predicate::Predicate;

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
		Command::GitRuntime(args) => git_runtime_supervisor::run(args, stdout, stderr),
		Command::Extract(args) => extract::run(args, stdout, stderr),
		Command::Stats(args) => stats::run(args, stdout, stderr),
		Command::Check(args) => check::run(args, stdout, stderr),
		Command::Diff(args) => diff::run(args, stdout, stderr),
		Command::Rules(args) => rules::run(args, stdout, stderr),
		Command::Docs(args) => docs::run(args, stdout, stderr),
		Command::Daemon(args) => daemon::run_daemon(args, stdout, stderr),
		Command::Query(args) => query::run(args, stdout, stderr),
		Command::Agent(args) => agent::run(args, stdout, stderr),
		#[cfg(feature = "mcp")]
		Command::Mcp(args) => mcp_command::run(args, stdout, stderr),
		Command::Langs(args) => langs::run(args, stdout, stderr),
		Command::Shapes(args) => shapes::run(args, stdout, stderr),
		Command::Manifest(args) => manifest::run(args, stdout, stderr),
	}
}
