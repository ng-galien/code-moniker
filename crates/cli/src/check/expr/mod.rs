//! Rule DSL for `code-moniker check`. Full reference: docs/cli/check-dsl.md.

mod ast;
mod atom;
mod collection;
mod cursor;
mod domain;
mod error;
mod metrics;
mod number;
mod pairs;
mod parse;
mod value;

pub(in crate::check) use ast::*;
pub use error::ParseError;
pub(in crate::check) use parse::parse;

#[cfg(test)]
mod snapshots;

#[cfg(test)]
mod test_support {
	pub(super) const TS: &str = "code+moniker://";
	pub(super) const KINDS: &[&str] = &["class", "method", "function", "module", "field", "param"];
}
