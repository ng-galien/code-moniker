//! Live rules engine for agent harnesses. See `docs/cli/check.md`.

pub(crate) mod command;
pub(crate) mod config;
pub(crate) mod eval;
pub(crate) mod exclude;
pub(crate) mod expr;
pub(crate) mod path;
pub(crate) mod suppress;
#[cfg(feature = "tui")]
pub mod workspace;

pub(crate) use config::Config;
#[cfg(test)]
pub(crate) use eval::evaluate;
pub(crate) use eval::{
	CompiledRules, RuleReport, Violation, compile_rules, evaluate_compiled, rule_report_compiled,
};
pub(in crate::check) use eval::{
	RequirementResolver, evaluate_compiled_with_requirements,
	rule_report_compiled_with_requirements,
};
pub(crate) use exclude::UriExclusionMatcher;
pub(crate) use suppress::apply as apply_suppressions;
