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
mod test_support {
	use super::*;

	pub(super) const TS: &str = "code+moniker://";
	pub(super) const KINDS: &[&str] = &["class", "method", "function", "module", "field"];

	pub(super) fn solo(e: &Expr) -> &Atom {
		match &e.root {
			Node::Atom(a) => a,
			other => panic!("expected solo Atom, got {other:?}"),
		}
	}

	pub(super) fn and_arms(e: &Expr) -> Vec<&Atom> {
		match &e.root {
			Node::And(children) => children
				.iter()
				.map(|c| match c {
					Node::Atom(a) => a,
					other => panic!("expected Atom under And, got {other:?}"),
				})
				.collect(),
			Node::Atom(a) => vec![a],
			other => panic!("expected And or Atom root, got {other:?}"),
		}
	}
}
