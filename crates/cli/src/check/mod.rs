//! Live rules engine for agent harnesses. See `docs/cli/check.md`.

pub mod command;
pub mod config;
pub mod eval;
pub mod exclude;
pub mod expr;
pub mod path;
pub mod suppress;
#[cfg(feature = "tui")]
pub mod workspace;

pub use command::run;
pub use config::{
	Config, KindRules, LangRules, RuleSeverity, load_default, load_with_cli_default_rules,
	load_with_options, load_with_overrides,
};
pub use eval::{
	CompiledRuleSpec, CompiledRules, RequirementResolver, RuleReport, Violation, compile_rules,
	evaluate, evaluate_compiled, evaluate_compiled_with_requirements, rule_report_compiled,
};
pub(crate) use exclude::UriExclusionMatcher;
pub use suppress::apply as apply_suppressions;
