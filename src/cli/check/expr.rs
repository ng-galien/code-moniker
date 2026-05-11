//! Rule DSL for `code-moniker check`. Full reference: docs/CHECK_DSL.md.

use regex::Regex;

use crate::core::moniker::Moniker;
use crate::core::uri::{UriConfig, from_uri};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum Lhs {
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
	SourceName,
	SourceKind,
	SourceVisibility,
	SourceMoniker,
	TargetName,
	TargetKind,
	TargetVisibility,
	TargetMoniker,
	SegmentName,
	SegmentKind,
}

impl Lhs {
	pub(super) fn as_str(self) -> &'static str {
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

pub(super) const TWO_CHAR_OPS: &[&str] = &["<=", ">=", "!=", "=~", "!~", "<@", "@>", "?="];

#[derive(Debug, Clone)]
pub(super) enum LhsExpr {
	Attr(Lhs),
	Count {
		domain: Domain,
		filter: Option<Box<Node>>,
	},
	SegmentOf {
		scope: SegmentScope,
		kind: String,
	},
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum SegmentScope {
	Def,
	Source,
	Target,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum Domain {
	Children(String),
	Segments,
	OutRefs,
	InRefs,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum QuantKind {
	Any,
	All,
	None,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum Op {
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
	PathMatch,
}

#[derive(Debug, Clone)]
pub(super) enum Rhs {
	Number(u32),
	RegexStr(String),
	Moniker(Moniker),
	Str(String),
	PathPattern(super::path::Pattern),
	Projection(Lhs),
}

#[derive(Debug, Clone)]
pub(super) struct Atom {
	pub lhs: LhsExpr,
	pub op: Op,
	pub rhs: Rhs,
	pub raw: String,
	pub regex: Option<Regex>,
}

#[derive(Debug, Clone)]
pub(super) enum Node {
	Atom(Atom),
	And(Vec<Node>),
	Or(Vec<Node>),
	Not(Box<Node>),
	Implies(Box<Node>, Box<Node>),
	Quantifier {
		kind: QuantKind,
		domain: Domain,
		filter: Box<Node>,
	},
}

#[derive(Debug, Clone)]
pub(super) struct Expr {
	pub root: Node,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseError {
	#[error("expression `{expr}`: {msg}")]
	BadExpr { expr: String, msg: String },
}

pub(super) fn parse(input: &str, scheme: &str, allowed_kinds: &[&str]) -> Result<Expr, ParseError> {
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
		if let Some(q) = self.try_parse_quantifier()? {
			return Ok(q);
		}
		if let Some(atom) = self.try_parse_count_atom()? {
			return Ok(Node::Atom(atom));
		}
		if let Some(atom) = self.try_parse_segment_atom()? {
			return Ok(Node::Atom(atom));
		}
		let atom_end = self.find_atom_end();
		if atom_end == self.pos {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("expected atom at byte {}", self.pos),
			});
		}
		let atom_str = &self.input[self.pos..atom_end];
		let atom = parse_atom(atom_str, self.scheme, self.raw)?;
		self.pos = atom_end;
		Ok(Node::Atom(atom))
	}

	fn try_parse_segment_atom(&mut self) -> Result<Option<Atom>, ParseError> {
		self.skip_ws();
		let rest = &self.input[self.pos..];
		let (scope, prefix_len) = if rest.starts_with("source.segment(") {
			(SegmentScope::Source, "source.segment(".len())
		} else if rest.starts_with("target.segment(") {
			(SegmentScope::Target, "target.segment(".len())
		} else if rest.starts_with("segment(") {
			(SegmentScope::Def, "segment(".len())
		} else {
			return Ok(None);
		};
		// Disambiguate: `segment(...)` could be the segment-domain inside a
		// quantifier, but quantifier was already tried before. If we got here
		// from primary, it's a projection call.
		let raw_start = self.pos;
		self.pos += prefix_len;
		// Read up to closing `)`
		let bytes = self.input.as_bytes();
		let arg_start = self.pos;
		while self.pos < bytes.len() && bytes[self.pos] != b')' {
			self.pos += 1;
		}
		if self.pos == bytes.len() {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: "unclosed `segment(...)` projection".to_string(),
			});
		}
		let arg = self.input[arg_start..self.pos].trim();
		let kind = unquote(arg).to_string();
		if kind.is_empty() {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: "segment(<kind>) needs a kind argument".to_string(),
			});
		}
		self.pos += 1;
		self.skip_ws();
		let (op_str, op_len) = self.eat_op().ok_or_else(|| ParseError::BadExpr {
			expr: self.raw.to_string(),
			msg: format!(
				"expected `<op> <rhs>` after `segment(...)` at byte {}",
				self.pos
			),
		})?;
		self.pos += op_len;
		let op = parse_op(op_str, self.raw)?;
		// Parse the RHS — re-use the existing rhs scanner. RHS ends at next
		// boundary / closing paren / EOI.
		let rhs_end = self.find_atom_end();
		let rhs_str = self.input[self.pos..rhs_end].trim();
		if rhs_str.is_empty() {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: "empty RHS after `segment(...)` op".to_string(),
			});
		}
		let rhs = parse_rhs(rhs_str, op, self.scheme, self.raw)?;
		let regex = match (&op, &rhs) {
			(Op::RegexMatch | Op::RegexNoMatch, Rhs::RegexStr(p)) => {
				Some(Regex::new(p).map_err(|e| ParseError::BadExpr {
					expr: self.raw.to_string(),
					msg: format!("invalid regex `{p}`: {e}"),
				})?)
			}
			_ => None,
		};
		// Validate op for SegmentOf (string lhs)
		match op {
			Op::Eq | Op::Ne | Op::RegexMatch | Op::RegexNoMatch => {}
			_ => {
				return Err(ParseError::BadExpr {
					expr: self.raw.to_string(),
					msg: format!("operator {op:?} not valid for segment(...) projection"),
				});
			}
		}
		self.pos = rhs_end;
		let raw = self.input[raw_start..self.pos].to_string();
		Ok(Some(Atom {
			lhs: LhsExpr::SegmentOf { scope, kind },
			op,
			rhs,
			raw,
			regex,
		}))
	}

	fn try_parse_count_atom(&mut self) -> Result<Option<Atom>, ParseError> {
		self.skip_ws();
		if !self.input[self.pos..].starts_with("count(") {
			return Ok(None);
		}
		let raw_start = self.pos;
		self.pos += "count".len();
		let (domain, filter) = self.parse_quantifier_body()?;
		self.skip_ws();
		let (op_str, op_len) = self.eat_op().ok_or_else(|| ParseError::BadExpr {
			expr: self.raw.to_string(),
			msg: format!(
				"expected numeric comparison after `count(...)` at byte {}",
				self.pos
			),
		})?;
		self.pos += op_len;
		let op = parse_op(op_str, self.raw)?;
		self.skip_ws();
		let num_start = self.pos;
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len() && bytes[self.pos].is_ascii_digit() {
			self.pos += 1;
		}
		let num_str = &self.input[num_start..self.pos];
		let n: u32 = num_str.parse().map_err(|_| ParseError::BadExpr {
			expr: self.raw.to_string(),
			msg: format!(
				"expected number after `count(...) {op_str}` at byte {num_start}, got `{num_str}`"
			),
		})?;
		let raw = self.input[raw_start..self.pos].to_string();
		Ok(Some(Atom {
			lhs: LhsExpr::Count {
				domain,
				filter: filter.map(Box::new),
			},
			op,
			rhs: Rhs::Number(n),
			raw,
			regex: None,
		}))
	}

	fn try_parse_quantifier(&mut self) -> Result<Option<Node>, ParseError> {
		self.skip_ws();
		for (kw, qk) in [
			("any", QuantKind::Any),
			("all", QuantKind::All),
			("none", QuantKind::None),
		] {
			if let Some(rest) = self.input[self.pos..].strip_prefix(kw)
				&& rest.starts_with('(')
			{
				self.pos += kw.len(); // consume kw, leave the `(` for the body parser
				let (domain, filter) = self.parse_quantifier_body()?;
				let filter = filter.ok_or_else(|| ParseError::BadExpr {
					expr: self.raw.to_string(),
					msg: format!("`{kw}` requires a filter expression: `{kw}(<domain>, <expr>)`"),
				})?;
				return Ok(Some(Node::Quantifier {
					kind: qk,
					domain,
					filter: Box::new(filter),
				}));
			}
		}
		Ok(None)
	}

	fn parse_quantifier_body(&mut self) -> Result<(Domain, Option<Node>), ParseError> {
		if self.peek_byte() != Some(b'(') {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("expected `(` at byte {}", self.pos),
			});
		}
		self.pos += 1;
		// Domain ident: count up to `,` or `)` (no whitespace allowed inside).
		self.skip_ws();
		let start = self.pos;
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len()
			&& (bytes[self.pos].is_ascii_alphanumeric() || bytes[self.pos] == b'_')
		{
			self.pos += 1;
		}
		let domain_ident = self.input[start..self.pos].to_string();
		if domain_ident.is_empty() {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("expected domain identifier at byte {}", start),
			});
		}
		let domain = match domain_ident.as_str() {
			"segment" => Domain::Segments,
			"out_refs" => Domain::OutRefs,
			"in_refs" => Domain::InRefs,
			other => {
				if !self.allowed_kinds.contains(&other) {
					return Err(ParseError::BadExpr {
						expr: self.raw.to_string(),
						msg: format!(
							"unknown domain `{other}` (allowed: segment, out_refs, in_refs, or one of {})",
							self.allowed_kinds.join(", ")
						),
					});
				}
				Domain::Children(other.to_string())
			}
		};
		self.skip_ws();
		let filter = if self.peek_byte() == Some(b',') {
			self.pos += 1;
			let f = self.parse_expr()?;
			self.skip_ws();
			Some(f)
		} else {
			None
		};
		if self.peek_byte() != Some(b')') {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("missing `)` for quantifier at byte {}", self.pos),
			});
		}
		self.pos += 1;
		Ok((domain, filter))
	}

	/// Boundaries are " AND " / " OR " / " => " at paren depth 0 outside
	/// string literals, so op chars inside a regex / count() arg never split.
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

	fn eat_op(&self) -> Option<(&'static str, usize)> {
		let rest = &self.input[self.pos..];
		for op in TWO_CHAR_OPS {
			if rest.starts_with(op) {
				return Some((*op, op.len()));
			}
		}
		for op in ["<", ">", "=", "~"] {
			if rest.starts_with(op) {
				return Some((op, 1));
			}
		}
		None
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

fn parse_atom(input: &str, scheme: &str, full: &str) -> Result<Atom, ParseError> {
	let raw = input.trim().to_string();
	if let Some(atom) = parse_has_segment(&raw, full)? {
		return Ok(atom);
	}
	let (lhs_str, op_str, rhs_str) = split_atom(&raw, full)?;
	let lhs = parse_lhs(lhs_str, full)?;
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

fn projection_name_to_lhs(s: &str) -> Option<Lhs> {
	Some(match s {
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

/// Operator search restricted to the LHS prefix so an op char inside a regex
/// RHS (`^[a-z]+$`, `foo<=bar`) can't be mistaken for the main operator.
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

fn parse_lhs(s: &str, full: &str) -> Result<LhsExpr, ParseError> {
	if s.starts_with("count(") {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: "internal: count(...) reached parse_lhs; should be handled at primary level"
				.to_string(),
		});
	}
	match projection_name_to_lhs(s) {
		Some(lhs) => Ok(LhsExpr::Attr(lhs)),
		None => Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!("unknown lhs `{s}`"),
		}),
	}
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
		LhsExpr::Count { .. } => {
			return match op {
				Lt | Le | Gt | Ge | Eq | Ne => Ok(()),
				_ => Err(ParseError::BadExpr {
					expr: full.to_string(),
					msg: format!("count(...) only accepts numeric operators, got {op:?}"),
				}),
			};
		}
		LhsExpr::SegmentOf { .. } => {
			return match op {
				Eq | Ne | RegexMatch | RegexNoMatch => Ok(()),
				_ => Err(ParseError::BadExpr {
					expr: full.to_string(),
					msg: format!("segment(...) only accepts string operators, got {op:?}"),
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
			(
				LhsExpr::Count {
					domain: Domain::Children(k),
					filter: None,
				},
				Op::Le,
				Rhs::Number(20),
			) if k == "method" => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_count_with_filter() {
		let e = parse("count(method, name =~ ^get) <= 5", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op) {
			(
				LhsExpr::Count {
					domain: Domain::Children(k),
					filter: Some(_),
				},
				Op::Le,
			) if k == "method" => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_any_quantifier() {
		let e = parse("any(method, name = 'execute')", TS, KINDS).unwrap();
		match &e.root {
			Node::Quantifier {
				kind: QuantKind::Any,
				domain: Domain::Children(k),
				..
			} if k == "method" => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_all_quantifier_on_segment() {
		let e = parse("all(segment, segment.kind = 'module')", TS, KINDS).unwrap();
		match &e.root {
			Node::Quantifier {
				kind: QuantKind::All,
				domain: Domain::Segments,
				..
			} => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_none_quantifier_on_out_refs() {
		let e = parse("none(out_refs, kind = 'imports')", TS, KINDS).unwrap();
		match &e.root {
			Node::Quantifier {
				kind: QuantKind::None,
				domain: Domain::OutRefs,
				..
			} => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn rejects_quantifier_without_filter() {
		assert!(parse("any(method)", TS, KINDS).is_err());
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
				assert!(msg.contains("unknown domain"), "{msg}");
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
