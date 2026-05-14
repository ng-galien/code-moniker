//! Live rules engine for agent harnesses. See `docs/cli/check.md`.

pub mod config;
pub mod eval;
pub mod expr;
pub mod path;
pub mod suppress;

pub use config::{Config, KindRules, LangRules, load_default, load_with_overrides};
pub use eval::{
	CompiledRules, RuleReport, Violation, compile_rules, evaluate, evaluate_compiled,
	rule_report_compiled,
};
pub use suppress::apply as apply_suppressions;
