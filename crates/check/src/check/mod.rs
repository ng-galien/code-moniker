//! Live rules engine for agent checks. See `docs/cli/check.md`.

pub(crate) mod command;
pub(crate) mod config;
pub(crate) mod corpus;
pub(crate) mod eval;
pub(crate) mod exclude;
pub(crate) mod expr;
pub(crate) mod path;
pub(crate) mod suppress;
pub mod workspace;
pub mod workspace_eval;

pub(crate) use config::Config;
#[cfg(test)]
pub(crate) use eval::evaluate;
pub(in crate::check) use eval::{
	CompiledEvaluationInput, RequirementResolver, evaluate_and_report_compiled,
};
pub(crate) use eval::{CompiledRules, RuleReport, Violation, compile_rules, evaluate_compiled};
pub(crate) use exclude::UriExclusionMatcher;
pub(crate) use suppress::apply as apply_suppressions;
