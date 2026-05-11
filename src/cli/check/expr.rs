//! Tiny DSL for rule predicates and CLI `--where`.
//!
//! Grammar:
//! ```text
//! expr  := atom (" AND " atom)*
//! atom  := lhs op rhs
//! lhs   := IDENT | "count" "(" IDENT ")"
//! op    := "="  | "!="
//!        | "<"  | "<=" | ">" | ">="
//!        | "=~" | "!~"
//!        | "@>" | "<@" | "?="
//! rhs   := NUMBER | STRING | MONIKER_URI
//! ```
//!
//! `" AND "` (literal substring, surrounding spaces) is the only multi-atom
//! combinator. Regex RHS that need a literal " AND " must escape one of the
//! spaces (`\sAND\s` or `(?i)and`) — the parser is whitespace-sensitive.

use std::collections::HashMap;

use regex::Regex;

use crate::core::moniker::Moniker;
use crate::core::uri::{UriConfig, from_uri};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Lhs {
	Name,
	Lines,
	Kind,
	Visibility,
	Text,
	Moniker,
}

impl Lhs {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Name => "name",
			Self::Lines => "lines",
			Self::Kind => "kind",
			Self::Visibility => "visibility",
			Self::Text => "text",
			Self::Moniker => "moniker",
		}
	}
}

/// Two-character operator tokens, ordered by parse priority. Shared between
/// the DSL parser and the CLI `--where` reader so the two cannot drift.
pub const TWO_CHAR_OPS: &[&str] = &["<=", ">=", "!=", "=~", "!~", "<@", "@>", "?="];

#[derive(Debug, Clone)]
pub enum LhsExpr {
	Attr(Lhs),
	/// `count(<kind>)` — number of children of the given kind under this def
	/// (when this def is treated as a parent).
	CountChildren(String),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Op {
	Eq,
	Ne,
	Lt,
	Le,
	Gt,
	Ge,
	RegexMatch,
	RegexNoMatch,
	AncestorOf,
	DescendantOf,
	BindMatch,
}

#[derive(Debug, Clone)]
pub enum Rhs {
	Number(u32),
	/// Raw regex string preserved for messages; compiled in `Atom::compile`.
	RegexStr(String),
	Moniker(Moniker),
	Str(String),
}

#[derive(Debug, Clone)]
pub struct Atom {
	pub lhs: LhsExpr,
	pub op: Op,
	pub rhs: Rhs,
	pub raw: String,
	pub regex: Option<Regex>,
}

#[derive(Debug, Clone)]
pub struct Expr(pub Vec<Atom>);

#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseError {
	#[error("expression `{expr}`: {msg}")]
	BadExpr { expr: String, msg: String },
}

pub fn parse(input: &str, scheme: &str, allowed_kinds: &[&str]) -> Result<Expr, ParseError> {
	let raw = input.to_string();
	let parts: Vec<&str> = input.split(" AND ").collect();
	let mut atoms = Vec::with_capacity(parts.len());
	for part in parts {
		atoms.push(parse_atom(part, scheme, &raw, allowed_kinds)?);
	}
	Ok(Expr(atoms))
}

fn parse_atom(
	input: &str,
	scheme: &str,
	full: &str,
	allowed_kinds: &[&str],
) -> Result<Atom, ParseError> {
	let raw = input.trim().to_string();
	let (lhs_str, op_str, rhs_str) = split_atom(&raw, full)?;
	let lhs = parse_lhs(lhs_str, full, allowed_kinds)?;
	let op = parse_op(op_str, full)?;
	check_type(&lhs, op, full)?;
	let rhs = parse_rhs(rhs_str, op, scheme, full)?;
	let regex = match (&op, &rhs) {
		(Op::RegexMatch | Op::RegexNoMatch, Rhs::RegexStr(p)) => {
			Some(Regex::new(p).map_err(|e| ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("invalid regex `{p}`: {e}"),
			})?)
		}
		_ => None,
	};
	Ok(Atom {
		lhs,
		op,
		rhs,
		raw,
		regex,
	})
}

/// Operator search is restricted to the LHS prefix (an `[A-Za-z_]+` ident,
/// optionally followed by a parenthesised arg for `count(<kind>)`). The RHS
/// is never scanned for operator chars, so regexes like `^[a-z]+$` or
/// `<= 10` in a `text =~ …` predicate cannot be mistaken for the operator.
fn split_atom<'a>(s: &'a str, full: &str) -> Result<(&'a str, &'a str, &'a str), ParseError> {
	let bail = || ParseError::BadExpr {
		expr: full.to_string(),
		msg: format!("expected `<lhs> <op> <rhs>` in `{s}`"),
	};
	let bytes = s.as_bytes();
	let lhs_end = lhs_token_end(bytes).ok_or_else(bail)?;
	let after_lhs = s[lhs_end..].trim_start();
	let op_offset = s.len() - after_lhs.len();
	for op in TWO_CHAR_OPS {
		if let Some(rest) = after_lhs.strip_prefix(op) {
			let lhs = s[..lhs_end].trim();
			let rhs = rest.trim();
			if lhs.is_empty() || rhs.is_empty() {
				return Err(bail());
			}
			return Ok((lhs, &s[op_offset..op_offset + op.len()], rhs));
		}
	}
	for op in ['<', '>', '='] {
		if let Some(rest) = after_lhs.strip_prefix(op) {
			let lhs = s[..lhs_end].trim();
			let rhs = rest.trim();
			if lhs.is_empty() || rhs.is_empty() {
				return Err(bail());
			}
			return Ok((lhs, &s[op_offset..op_offset + op.len_utf8()], rhs));
		}
	}
	Err(bail())
}

fn lhs_token_end(bytes: &[u8]) -> Option<usize> {
	let mut i = 0;
	while i < bytes.len() && bytes[i].is_ascii_whitespace() {
		i += 1;
	}
	let start = i;
	while i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
		i += 1;
	}
	if i == start {
		return None;
	}
	if i < bytes.len() && bytes[i] == b'(' {
		i += 1;
		while i < bytes.len() && bytes[i] != b')' {
			i += 1;
		}
		if i == bytes.len() {
			return None;
		}
		i += 1;
	}
	Some(i)
}

fn parse_lhs(s: &str, full: &str, allowed_kinds: &[&str]) -> Result<LhsExpr, ParseError> {
	if let Some(rest) = s.strip_prefix("count(").and_then(|s| s.strip_suffix(")")) {
		let kind = rest.trim();
		if kind.is_empty() {
			return Err(ParseError::BadExpr {
				expr: full.to_string(),
				msg: "count() needs a kind argument".to_string(),
			});
		}
		if !allowed_kinds.contains(&kind) {
			return Err(ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!(
					"count(`{kind}`) — unknown kind for this language (allowed: {})",
					allowed_kinds.join(", ")
				),
			});
		}
		return Ok(LhsExpr::CountChildren(kind.to_string()));
	}
	let attr = match s {
		"name" => Lhs::Name,
		"lines" => Lhs::Lines,
		"kind" => Lhs::Kind,
		"visibility" => Lhs::Visibility,
		"text" => Lhs::Text,
		"moniker" => Lhs::Moniker,
		other => {
			return Err(ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("unknown lhs `{other}`"),
			});
		}
	};
	Ok(LhsExpr::Attr(attr))
}

fn parse_op(s: &str, full: &str) -> Result<Op, ParseError> {
	Ok(match s {
		"=" => Op::Eq,
		"!=" => Op::Ne,
		"<" => Op::Lt,
		"<=" => Op::Le,
		">" => Op::Gt,
		">=" => Op::Ge,
		"=~" => Op::RegexMatch,
		"!~" => Op::RegexNoMatch,
		"@>" => Op::AncestorOf,
		"<@" => Op::DescendantOf,
		"?=" => Op::BindMatch,
		other => {
			return Err(ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("unknown operator `{other}`"),
			});
		}
	})
}

fn check_type(lhs: &LhsExpr, op: Op, full: &str) -> Result<(), ParseError> {
	use Lhs::*;
	use Op::*;
	let lhs_attr = match lhs {
		LhsExpr::Attr(a) => *a,
		LhsExpr::CountChildren(_) => {
			return match op {
				Lt | Le | Gt | Ge | Eq | Ne => Ok(()),
				_ => Err(ParseError::BadExpr {
					expr: full.to_string(),
					msg: format!("count(...) only accepts numeric operators, got {op:?}"),
				}),
			};
		}
	};
	let ok = match (lhs_attr, op) {
		// String ops
		(Name | Kind | Visibility | Text, Eq | Ne | RegexMatch | RegexNoMatch) => true,
		// Numeric ops
		(Lines, Lt | Le | Gt | Ge | Eq | Ne) => true,
		// Moniker structural ops
		(Moniker, Eq | Ne | AncestorOf | DescendantOf | BindMatch) => true,
		_ => false,
	};
	if !ok {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!("operator {op:?} not valid for lhs {lhs_attr:?}"),
		});
	}
	Ok(())
}

fn parse_rhs(s: &str, op: Op, scheme: &str, full: &str) -> Result<Rhs, ParseError> {
	let s = s.trim();
	let s = if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
		|| (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
	{
		&s[1..s.len() - 1]
	} else {
		s
	};
	Ok(match op {
		Op::RegexMatch | Op::RegexNoMatch => Rhs::RegexStr(s.to_string()),
		Op::AncestorOf | Op::DescendantOf | Op::BindMatch => {
			let cfg = UriConfig { scheme };
			let m = from_uri(s, &cfg).map_err(|e| ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("invalid moniker URI `{s}`: {e}"),
			})?;
			Rhs::Moniker(m)
		}
		Op::Lt | Op::Le | Op::Gt | Op::Ge => {
			let n: u32 = s.parse().map_err(|_| ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("expected number, got `{s}`"),
			})?;
			Rhs::Number(n)
		}
		Op::Eq | Op::Ne => {
			if let Ok(n) = s.parse::<u32>() {
				Rhs::Number(n)
			} else if s.contains("+moniker://") {
				let cfg = UriConfig { scheme };
				let m = from_uri(s, &cfg).map_err(|e| ParseError::BadExpr {
					expr: full.to_string(),
					msg: format!("invalid moniker URI `{s}`: {e}"),
				})?;
				Rhs::Moniker(m)
			} else {
				Rhs::Str(s.to_string())
			}
		}
	})
}

/// Free variables available to a template message — set by the evaluator
/// when the atom fires. Keyed by short name (`name`, `value`, `expected`, …).
pub type Bindings = HashMap<&'static str, String>;

#[cfg(test)]
mod tests {
	use super::*;

	const TS: &str = "ts+moniker://";
	const KINDS: &[&str] = &["class", "method", "function", "module"];

	#[test]
	fn parses_name_regex() {
		let e = parse("name =~ ^[A-Z]", TS, KINDS).unwrap();
		assert_eq!(e.0.len(), 1);
		let a = &e.0[0];
		assert!(matches!(a.lhs, LhsExpr::Attr(Lhs::Name)));
		assert!(matches!(a.op, Op::RegexMatch));
		assert!(matches!(a.rhs, Rhs::RegexStr(_)));
		assert!(a.regex.is_some());
	}

	#[test]
	fn parses_lines_le() {
		let e = parse("lines <= 60", TS, KINDS).unwrap();
		match (&e.0[0].lhs, &e.0[0].op, &e.0[0].rhs) {
			(LhsExpr::Attr(Lhs::Lines), Op::Le, Rhs::Number(60)) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_moniker_descendant() {
		let e = parse("moniker <@ ts+moniker://./class:Foo", TS, KINDS).unwrap();
		match (&e.0[0].lhs, &e.0[0].op, &e.0[0].rhs) {
			(LhsExpr::Attr(Lhs::Moniker), Op::DescendantOf, Rhs::Moniker(_)) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_count() {
		let e = parse("count(method) <= 20", TS, KINDS).unwrap();
		match (&e.0[0].lhs, &e.0[0].op, &e.0[0].rhs) {
			(LhsExpr::CountChildren(k), Op::Le, Rhs::Number(20)) if k == "method" => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_and_combination() {
		let e = parse("name =~ ^[A-Z] AND lines <= 60", TS, KINDS).unwrap();
		assert_eq!(e.0.len(), 2);
	}

	#[test]
	fn rejects_op_lhs_type_mismatch() {
		let r = parse("lines =~ foo", TS, KINDS);
		assert!(r.is_err(), "lines is numeric, =~ should be rejected");
	}

	#[test]
	fn rejects_unknown_lhs() {
		let r = parse("bogus = foo", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn rejects_count_with_regex_op() {
		let r = parse("count(method) =~ foo", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn rejects_count_kind_typo() {
		let r = parse("count(methdo) <= 20", TS, KINDS);
		match r {
			Err(ParseError::BadExpr { msg, .. }) => {
				assert!(msg.contains("methdo"), "{msg}");
				assert!(msg.contains("unknown kind"), "{msg}");
			}
			other => panic!("expected BadExpr, got {other:?}"),
		}
	}

	#[test]
	fn rejects_invalid_regex() {
		let r = parse("name =~ [unclosed", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn rejects_invalid_moniker_uri() {
		let r = parse("moniker <@ not-a-uri", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn rejects_non_numeric_for_lines() {
		let r = parse("lines <= forty", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn regex_rhs_containing_op_chars_is_not_split_on_rhs() {
		// RHS contains `>=` and `<=` — must NOT be taken as the main op.
		let e = parse("text =~ ^count\\(.+\\) <= 20$", TS, KINDS).unwrap();
		assert_eq!(e.0.len(), 1);
		match (&e.0[0].lhs, &e.0[0].op) {
			(LhsExpr::Attr(Lhs::Text), Op::RegexMatch) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn regex_rhs_with_neq_token_is_not_split_on_rhs() {
		let e = parse("text =~ foo!=bar", TS, KINDS).unwrap();
		assert!(matches!(e.0[0].op, Op::RegexMatch));
		match &e.0[0].rhs {
			Rhs::RegexStr(s) => assert_eq!(s, "foo!=bar"),
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn strips_surrounding_quotes_on_rhs() {
		let e = parse("name =~ \"^foo$\"", TS, KINDS).unwrap();
		match &e.0[0].rhs {
			Rhs::RegexStr(s) => assert_eq!(s, "^foo$"),
			other => panic!("unexpected: {other:?}"),
		}
	}
}
