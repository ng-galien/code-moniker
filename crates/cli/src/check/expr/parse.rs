use super::ast::*;
use super::atom::{build_atom, parse_atom, parse_op, parse_rhs, unquote};
use super::cursor::Parser;
use super::error::ParseError;

pub(in crate::check) fn parse(
	input: &str,
	scheme: &str,
	allowed_kinds: &[&str],
) -> Result<Expr, ParseError> {
	let raw = input.to_string();
	let mut p = Parser::new(input, scheme, allowed_kinds, &raw);
	let root = p.parse_expr()?;
	p.skip_ws();
	if p.pos < p.input.len() {
		let msg = format!("trailing input at byte {}: `{}`", p.pos, &p.input[p.pos..]);
		return Err(ParseError::BadExpr { expr: raw, msg });
	}
	Ok(Expr { root })
}

impl<'a> Parser<'a> {
	pub(super) fn parse_expr(&mut self) -> Result<Node, ParseError> {
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
		if let Some(atom) = self.try_parse_collection_subset_atom()? {
			return Ok(Node::Atom(atom));
		}
		if let Some(atom) = self.try_parse_number_atom()? {
			return Ok(Node::Atom(atom));
		}
		if let Some(atom) = self.try_parse_mode_atom()? {
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
		let atom = parse_atom(atom_str, self.scheme, self.allowed_kinds, self.raw)?;
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
		let raw_start = self.pos;
		self.pos += prefix_len;
		let arg = self
			.take_until_byte(b')')
			.ok_or_else(|| ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: "unclosed `segment(...)` projection".to_string(),
			})?
			.trim();
		let kind = unquote(arg).to_string();
		if kind.is_empty() {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: "segment(<kind>) needs a kind argument".to_string(),
			});
		}
		self.pos += 1;
		Ok(Some(self.parse_comparison_tail(
			raw_start,
			LhsExpr::SegmentOf { scope, kind },
			"expected `<op> <rhs>` after `segment(...)`",
		)?))
	}

	fn try_parse_number_atom(&mut self) -> Result<Option<Atom>, ParseError> {
		self.skip_ws();
		if !self.next_starts_number_call() {
			return Ok(None);
		}
		let raw_start = self.pos;
		let lhs = self.parse_number_expr()?;
		Ok(Some(self.parse_comparison_tail(
			raw_start,
			LhsExpr::Number(lhs),
			"expected numeric comparison after number expression",
		)?))
	}

	fn try_parse_mode_atom(&mut self) -> Result<Option<Atom>, ParseError> {
		self.skip_ws();
		if !self.input[self.pos..].starts_with("mode(") {
			return Ok(None);
		}
		let raw_start = self.pos;
		let lhs = self.parse_mode_lhs()?;
		Ok(Some(self.parse_comparison_tail(
			raw_start,
			lhs,
			"expected comparison after `mode(...)`",
		)?))
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
				self.pos += kw.len();
				let (domain, filter) = self.parse_domain_filter_body(Parser::parse_expr)?;
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

	fn parse_comparison_tail(
		&mut self,
		raw_start: usize,
		lhs: LhsExpr,
		missing_op_msg: &str,
	) -> Result<Atom, ParseError> {
		self.skip_ws();
		let (op_str, op_len) = self.eat_op().ok_or_else(|| ParseError::BadExpr {
			expr: self.raw.to_string(),
			msg: format!("{missing_op_msg} at byte {}", self.pos),
		})?;
		self.pos += op_len;
		let op = parse_op(op_str, self.raw)?;
		self.skip_ws();
		let rhs_end = self.find_atom_end();
		let rhs_str = self.input[self.pos..rhs_end].trim();
		if rhs_str.is_empty() {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: "empty RHS after comparison op".to_string(),
			});
		}
		let rhs = parse_rhs(rhs_str, op, self.scheme, self.allowed_kinds, self.raw)?;
		self.pos = rhs_end;
		let raw = self.input[raw_start..self.pos].to_string();
		build_atom(lhs, op, rhs, raw, self.raw)
	}
}

#[cfg(test)]
mod tests {
	use super::super::test_support::{KINDS, TS, and_arms};
	use super::super::*;
	use super::parse;

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
		let e = parse("(name = 'X' OR name = 'Y') AND lines <= 10", TS, KINDS).unwrap();
		assert!(matches!(e.root, Node::And(_)));
	}

	#[test]
	fn precedence_implies_is_lowest() {
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
}
