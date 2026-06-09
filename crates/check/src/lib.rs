//! Rules engine for code-moniker. Owns the rule DSL, rule configuration and
//! profiles, evaluation over the symbol graph, suppression, and the file/project
//! scan pipeline. Produces structured [`FileReport`]/[`FileError`] values; it
//! does not render output or own process exit codes — that is the CLI's job.

mod check;

pub use check::command::{
	CheckRequest, CheckRun, CheckSkipReason, CheckSummary, DefaultRulesSelection,
	FailedRuleSummary, FileError, FileReport, RuleSetRequest, SourceReport, ViolationCounts,
	check_graph_with_config, check_one_file, check_project, check_project_files,
	check_source_with_config, compiled_specs_with_config,
};
pub use check::config::{Config, RuleSeverity, load_with_cli_default_rules, load_with_overrides};
pub use check::eval::{
	CompiledRuleSpec, CompiledRules, RuleReport, Violation, compile_rules, evaluate_compiled,
	rule_report_compiled,
};
pub use check::exclude::UriExclusionMatcher;
pub use check::suppress::apply as apply_suppressions;

pub use check::workspace;
