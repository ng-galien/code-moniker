//! Live rules engine for agent harnesses. See `docs/cli/check.md`.

pub(crate) mod command;
pub(crate) mod config;
pub(crate) mod eval;
pub(crate) mod exclude;
pub(crate) mod expr;
pub(crate) mod path;
pub(crate) mod suppress;
#[cfg(feature = "tui")]
pub(crate) mod workspace;

pub(crate) use command::run;
pub(crate) use config::{Config, RuleSeverity, load_with_cli_default_rules, load_with_overrides};
#[cfg(test)]
pub(crate) use eval::evaluate;
pub(crate) use eval::{
	CompiledRuleSpec, CompiledRules, RequirementResolver, RuleReport, Violation, compile_rules,
	evaluate_compiled, evaluate_compiled_with_requirements, rule_report_compiled,
};
pub(crate) use exclude::UriExclusionMatcher;
pub(crate) use suppress::apply as apply_suppressions;
