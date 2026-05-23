//! Live rules engine for agent harnesses. See `docs/cli/check.md`.

pub mod config;
pub mod eval;
pub mod exclude;
pub mod expr;
pub mod path;
pub mod suppress;

pub use config::{
	Config, KindRules, LangRules, RuleSeverity, load_default, load_with_cli_default_rules,
	load_with_options, load_with_overrides,
};
pub use eval::{
	CompiledRuleSpec, CompiledRules, RuleReport, Violation, compile_rules, evaluate,
	evaluate_compiled, rule_report_compiled,
};
pub use exclude::UriExclusionMatcher;
pub use suppress::apply as apply_suppressions;
