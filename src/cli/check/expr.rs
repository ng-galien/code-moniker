//! Rule DSL for `code-moniker check`. Full reference: docs/CHECK_DSL.md.
//!
//! Boolean tree over atoms: `AND` `OR` `NOT` `=>` with parens. Precedence
//! `=>` < `OR` < `AND` < `NOT`. Atom = `<lhs> <op> <rhs>` ; ops mirror the
//! moniker algebra plus regex (`=~` `!~`) and numeric comparison.
//!
//! Operator search is restricted to the LHS prefix (an ident, optionally
//! `count(<kind>)`), so a regex RHS like `^count\(.+\) <= 20$` cannot be
//! mis-split. Boundaries between atoms are `" AND "`, `" OR "`, `" => "`
//! at depth 0 outside string literals.

use std::collections::HashMap;

use regex::Regex;

use crate::core::moniker::Moniker;
use crate::core::uri::{UriConfig, from_uri};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Lhs {
	// Def scope (and unprefixed in ref scope: `kind` = ref_kind)
	Name,
	Lines,
	Kind,
	Visibility,
	Text,
	Moniker,
	Depth,
	Confidence,
	ParentName,
	ParentKind,
	// Ref scope projections
	SourceName,
	SourceKind,
	SourceVisibility,
	SourceMoniker,
	TargetName,
	TargetKind,
	TargetVisibility,
	TargetMoniker,
	// Segment-domain quantifier body
	SegmentName,
	SegmentKind,
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
			Self::Depth => "depth",
			Self::Confidence => "confidence",
			Self::ParentName => "parent.name",
			Self::ParentKind => "parent.kind",
			Self::SourceName => "source.name",
			Self::SourceKind => "source.kind",
			Self::SourceVisibility => "source.visibility",
			Self::SourceMoniker => "source",
			Self::TargetName => "target.name",
			Self::TargetKind => "target.kind",
			Self::TargetVisibility => "target.visibility",
			Self::TargetMoniker => "target",
			Self::SegmentName => "segment.name",
			Self::SegmentKind => "segment.kind",
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
	/// `~ '<path-pattern>'` — moniker matches the path pattern.
	PathMatch,
}

#[derive(Debug, Clone)]
pub enum Rhs {
	Number(u32),
	/// Raw regex string preserved for messages; compiled in `Atom::compile`.
	RegexStr(String),
	Moniker(Moniker),
	Str(String),
	PathPattern(super::path::Pattern),
	/// Another projection on the same scope — resolved at eval time. Enables
	/// `name != parent.name`, `source.kind = target.kind`, …
	Projection(Lhs),
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
pub enum Node {
	Atom(Atom),
	And(Vec<Node>),
	Or(Vec<Node>),
	Not(Box<Node>),
	Implies(Box<Node>, Box<Node>),
}

#[derive(Debug, Clone)]
pub struct Expr {
	pub root: Node,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseError {
	#[error("expression `{expr}`: {msg}")]
	BadExpr { expr: String, msg: String },
}

pub fn parse(input: &str, scheme: &str, allowed_kinds: &[&str]) -> Result<Expr, ParseError> {
	let raw = input.to_string();
	let mut p = Parser {
		input,
		pos: 0,
		scheme,
		allowed_kinds,
		raw: &raw,
	};
	let root = p.parse_expr()?;
	p.skip_ws();
	if p.pos < p.input.len() {
		let msg = format!("trailing input at byte {}: `{}`", p.pos, &p.input[p.pos..]);
		return Err(ParseError::BadExpr { expr: raw, msg });
	}
	Ok(Expr { root })
}

struct Parser<'a> {
	input: &'a str,
	pos: usize,
	scheme: &'a str,
	allowed_kinds: &'a [&'a str],
	raw: &'a str,
}

impl<'a> Parser<'a> {
	fn parse_expr(&mut self) -> Result<Node, ParseError> {
		let lhs = self.parse_or()?;
		self.skip_ws();
		if self.eat_keyword("=>") {
			let rhs = self.parse_or()?;
			return Ok(Node::Implies(Box::new(lhs), Box::new(rhs)));
		}
		Ok(lhs)
	}

	fn parse_or(&mut self) -> Result<Node, ParseError> {
		let mut nodes = vec![self.parse_and()?];
		loop {
			self.skip_ws();
			if !self.eat_keyword("OR") {
				break;
			}
			nodes.push(self.parse_and()?);
		}
		Ok(if nodes.len() == 1 {
			nodes.pop().unwrap()
		} else {
			Node::Or(nodes)
		})
	}

	fn parse_and(&mut self) -> Result<Node, ParseError> {
		let mut nodes = vec![self.parse_not()?];
		loop {
			self.skip_ws();
			if !self.eat_keyword("AND") {
				break;
			}
			nodes.push(self.parse_not()?);
		}
		Ok(if nodes.len() == 1 {
			nodes.pop().unwrap()
		} else {
			Node::And(nodes)
		})
	}

	fn parse_not(&mut self) -> Result<Node, ParseError> {
		self.skip_ws();
		if self.eat_keyword("NOT") {
			let inner = self.parse_not()?;
			return Ok(Node::Not(Box::new(inner)));
		}
		self.parse_primary()
	}

	fn parse_primary(&mut self) -> Result<Node, ParseError> {
		self.skip_ws();
		if self.peek_byte() == Some(b'(') {
			self.pos += 1;
			let inner = self.parse_expr()?;
			self.skip_ws();
			if self.peek_byte() != Some(b')') {
				return Err(ParseError::BadExpr {
					expr: self.raw.to_string(),
					msg: format!("missing `)` at byte {}", self.pos),
				});
			}
			self.pos += 1;
			return Ok(inner);
		}
		let atom_end = self.find_atom_end();
		if atom_end == self.pos {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("expected atom at byte {}", self.pos),
			});
		}
		let atom_str = &self.input[self.pos..atom_end];
		let atom = parse_atom(atom_str, self.scheme, self.raw, self.allowed_kinds)?;
		self.pos = atom_end;
		Ok(Node::Atom(atom))
	}

	/// Walk from `self.pos` to the next top-level boolean connective, closing
	/// paren, or EOI. Tracks paren depth and string/regex literals so an
	/// operator char inside a regex or a `count(...)` argument can't end the
	/// atom prematurely.
	fn find_atom_end(&self) -> usize {
		let bytes = self.input.as_bytes();
		let mut i = self.pos;
		let mut depth: i32 = 0;
		let mut in_string: Option<u8> = None;
		while i < bytes.len() {
			let c = bytes[i];
			if let Some(q) = in_string {
				if c == q {
					in_string = None;
				}
				i += 1;
				continue;
			}
			match c {
				b'\'' | b'"' => {
					in_string = Some(c);
					i += 1;
				}
				b'(' => {
					depth += 1;
					i += 1;
				}
				b')' => {
					if depth == 0 {
						return i;
					}
					depth -= 1;
					i += 1;
				}
				_ => {
					if depth == 0 && self.boundary_at(i) {
						return i;
					}
					i += 1;
				}
			}
		}
		i
	}

	fn boundary_at(&self, i: usize) -> bool {
		let rest = &self.input[i..];
		rest.starts_with(" AND ")
			|| rest.starts_with(" OR ")
			|| rest.starts_with(" => ")
			|| rest.starts_with(" AND\t")
			|| rest.starts_with(" OR\t")
			|| rest.starts_with(" =>\t")
	}

	fn skip_ws(&mut self) {
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
			self.pos += 1;
		}
	}

	fn peek_byte(&self) -> Option<u8> {
		self.input.as_bytes().get(self.pos).copied()
	}

	fn eat_keyword(&mut self, kw: &str) -> bool {
		let rest = &self.input[self.pos..];
		if let Some(after) = rest.strip_prefix(kw) {
			let next_ok = after.is_empty()
				|| after.starts_with(|c: char| c.is_ascii_whitespace())
				|| after.starts_with('(');
			if next_ok {
				self.pos += kw.len();
				return true;
			}
		}
		false
	}
}

fn parse_atom(
	input: &str,
	scheme: &str,
	full: &str,
	allowed_kinds: &[&str],
) -> Result<Atom, ParseError> {
	let raw = input.trim().to_string();
	if let Some(atom) = parse_has_segment(&raw, full)? {
		return Ok(atom);
	}
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

/// `has_segment("kind", "name")` is sugar for `moniker ~ '**/kind:name/**'`.
fn parse_has_segment(raw: &str, full: &str) -> Result<Option<Atom>, ParseError> {
	let Some(args) = raw
		.strip_prefix("has_segment(")
		.and_then(|s| s.strip_suffix(')'))
	else {
		return Ok(None);
	};
	let bail = |msg: String| ParseError::BadExpr {
		expr: full.to_string(),
		msg,
	};
	let mut parts = args.splitn(2, ',').map(str::trim);
	let kind = parts
		.next()
		.ok_or_else(|| bail("has_segment(kind, name) needs two args".to_string()))?;
	let name = parts
		.next()
		.ok_or_else(|| bail("has_segment(kind, name) needs two args".to_string()))?;
	let kind = unquote(kind);
	let name = unquote(name);
	if kind.is_empty() || name.is_empty() {
		return Err(bail(
			"has_segment(kind, name) args must be non-empty strings".to_string(),
		));
	}
	let pat_src = format!("**/{kind}:{name}/**");
	let pattern = super::path::parse(&pat_src).map_err(|e| bail(format!("{e}")))?;
	Ok(Some(Atom {
		lhs: LhsExpr::Attr(Lhs::Moniker),
		op: Op::PathMatch,
		rhs: Rhs::PathPattern(pattern),
		raw: raw.to_string(),
		regex: None,
	}))
}

/// Recognize projection names usable as RHS (e.g. `parent.name`, `target.kind`).
/// Returns the matching `Lhs` variant or `None` if `s` isn't a known projection.
fn projection_name_to_lhs(s: &str) -> Option<Lhs> {
	Some(match s {
		"name" => Lhs::Name,
		"kind" => Lhs::Kind,
		"visibility" => Lhs::Visibility,
		"text" => Lhs::Text,
		"moniker" => Lhs::Moniker,
		"depth" => Lhs::Depth,
		"confidence" => Lhs::Confidence,
		"parent.name" => Lhs::ParentName,
		"parent.kind" => Lhs::ParentKind,
		"source.name" => Lhs::SourceName,
		"source.kind" => Lhs::SourceKind,
		"source.visibility" => Lhs::SourceVisibility,
		"target.name" => Lhs::TargetName,
		"target.kind" => Lhs::TargetKind,
		"target.visibility" => Lhs::TargetVisibility,
		"segment.name" => Lhs::SegmentName,
		"segment.kind" => Lhs::SegmentKind,
		_ => return None,
	})
}

fn unquote(s: &str) -> &str {
	let s = s.trim();
	if (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
		|| (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
	{
		&s[1..s.len() - 1]
	} else {
		s
	}
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
	for op in ['<', '>', '=', '~'] {
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
	while i < bytes.len()
		&& (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'.')
	{
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
		"depth" => Lhs::Depth,
		"confidence" => Lhs::Confidence,
		"parent.name" => Lhs::ParentName,
		"parent.kind" => Lhs::ParentKind,
		"source" => Lhs::SourceMoniker,
		"source.name" => Lhs::SourceName,
		"source.kind" => Lhs::SourceKind,
		"source.visibility" => Lhs::SourceVisibility,
		"target" => Lhs::TargetMoniker,
		"target.name" => Lhs::TargetName,
		"target.kind" => Lhs::TargetKind,
		"target.visibility" => Lhs::TargetVisibility,
		"segment.name" => Lhs::SegmentName,
		"segment.kind" => Lhs::SegmentKind,
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
		"~" => Op::PathMatch,
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
		(
			Name | Kind | Visibility | Text | Confidence | ParentName | ParentKind | SourceName
			| SourceKind | SourceVisibility | TargetName | TargetKind | TargetVisibility
			| SegmentName | SegmentKind,
			Eq | Ne | RegexMatch | RegexNoMatch,
		) => true,
		// Numeric ops
		(Lines | Depth, Lt | Le | Gt | Ge | Eq | Ne) => true,
		// Moniker structural ops (incl. source/target as monikers)
		(
			Moniker | SourceMoniker | TargetMoniker,
			Eq | Ne | AncestorOf | DescendantOf | BindMatch | PathMatch,
		) => true,
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
		Op::PathMatch => {
			let pattern = super::path::parse(s).map_err(|e| ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("{e}"),
			})?;
			Rhs::PathPattern(pattern)
		}
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
			} else if let Some(lhs) = projection_name_to_lhs(s) {
				Rhs::Projection(lhs)
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

	fn solo(e: &Expr) -> &Atom {
		match &e.root {
			Node::Atom(a) => a,
			other => panic!("expected solo Atom, got {other:?}"),
		}
	}

	fn and_arms(e: &Expr) -> Vec<&Atom> {
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

	#[test]
	fn parses_name_regex() {
		let e = parse("name =~ ^[A-Z]", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.lhs, LhsExpr::Attr(Lhs::Name)));
		assert!(matches!(a.op, Op::RegexMatch));
		assert!(matches!(a.rhs, Rhs::RegexStr(_)));
		assert!(a.regex.is_some());
	}

	#[test]
	fn parses_lines_le() {
		let e = parse("lines <= 60", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(LhsExpr::Attr(Lhs::Lines), Op::Le, Rhs::Number(60)) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_moniker_descendant() {
		let e = parse("moniker <@ ts+moniker://./class:Foo", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(LhsExpr::Attr(Lhs::Moniker), Op::DescendantOf, Rhs::Moniker(_)) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_count() {
		let e = parse("count(method) <= 20", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(LhsExpr::CountChildren(k), Op::Le, Rhs::Number(20)) if k == "method" => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_and_combination() {
		let e = parse("name =~ ^[A-Z] AND lines <= 60", TS, KINDS).unwrap();
		assert_eq!(and_arms(&e).len(), 2);
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
		let a = solo(&e);
		match (&a.lhs, &a.op) {
			(LhsExpr::Attr(Lhs::Text), Op::RegexMatch) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn regex_rhs_with_neq_token_is_not_split_on_rhs() {
		let e = parse("text =~ foo!=bar", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.op, Op::RegexMatch));
		match &a.rhs {
			Rhs::RegexStr(s) => assert_eq!(s, "foo!=bar"),
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn strips_surrounding_quotes_on_rhs() {
		let e = parse("name =~ \"^foo$\"", TS, KINDS).unwrap();
		match &solo(&e).rhs {
			Rhs::RegexStr(s) => assert_eq!(s, "^foo$"),
			other => panic!("unexpected: {other:?}"),
		}
	}

	// ─── booleans + implication ─────────────────────────────────────────

	#[test]
	fn parses_or() {
		let e = parse("name = 'Foo' OR name = 'Bar'", TS, KINDS).unwrap();
		match &e.root {
			Node::Or(children) => assert_eq!(children.len(), 2),
			other => panic!("expected Or, got {other:?}"),
		}
	}

	#[test]
	fn parses_not() {
		let e = parse("NOT name = 'Foo'", TS, KINDS).unwrap();
		assert!(matches!(e.root, Node::Not(_)));
	}

	#[test]
	fn parses_implies() {
		let e = parse("name = 'Foo' => kind = 'class'", TS, KINDS).unwrap();
		assert!(matches!(e.root, Node::Implies(_, _)));
	}

	#[test]
	fn parses_parens_override_precedence() {
		// `A OR B AND C` would normally bind as `A OR (B AND C)`.
		// `(A OR B) AND C` must produce an And at the root.
		let e = parse("(name = 'X' OR name = 'Y') AND lines <= 10", TS, KINDS).unwrap();
		assert!(matches!(e.root, Node::And(_)));
	}

	#[test]
	fn precedence_implies_is_lowest() {
		// `A OR B => C AND D` ≡ `(A OR B) => (C AND D)`
		let e = parse(
			"name = 'X' OR name = 'Y' => lines <= 10 AND kind = 'class'",
			TS,
			KINDS,
		)
		.unwrap();
		match e.root {
			Node::Implies(lhs, rhs) => {
				assert!(matches!(*lhs, Node::Or(_)));
				assert!(matches!(*rhs, Node::And(_)));
			}
			other => panic!("expected Implies at root, got {other:?}"),
		}
	}

	#[test]
	fn precedence_not_binds_tighter_than_and() {
		// `NOT A AND B` ≡ `(NOT A) AND B`
		let e = parse("NOT name = 'X' AND lines <= 10", TS, KINDS).unwrap();
		match e.root {
			Node::And(children) => {
				assert!(matches!(children[0], Node::Not(_)));
				assert!(matches!(children[1], Node::Atom(_)));
			}
			other => panic!("expected And, got {other:?}"),
		}
	}

	#[test]
	fn rejects_unmatched_paren() {
		assert!(parse("(name = 'X'", TS, KINDS).is_err());
		assert!(parse("name = 'X')", TS, KINDS).is_err());
	}

	// ─── path patterns ──────────────────────────────────────────────────

	#[test]
	fn parses_path_match() {
		let e = parse("moniker ~ '**/class:Foo/**'", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.op, Op::PathMatch));
		assert!(matches!(a.rhs, Rhs::PathPattern(_)));
	}

	#[test]
	fn parses_path_match_with_regex_step() {
		let e = parse("moniker ~ '**/class:/Port$/'", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.op, Op::PathMatch));
	}

	#[test]
	fn has_segment_desugars_to_path_match() {
		let e = parse("has_segment('module', 'domain')", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.op, Op::PathMatch));
		match &a.rhs {
			Rhs::PathPattern(p) => assert_eq!(p.raw, "**/module:domain/**"),
			other => panic!("expected PathPattern, got {other:?}"),
		}
	}

	#[test]
	fn rejects_path_match_on_non_moniker_lhs() {
		assert!(parse("name ~ 'foo'", TS, KINDS).is_err());
	}

	#[test]
	fn rejects_invalid_path_pattern() {
		assert!(parse("moniker ~ ''", TS, KINDS).is_err());
		assert!(parse("moniker ~ 'no-colon-step'", TS, KINDS).is_err());
	}
}
