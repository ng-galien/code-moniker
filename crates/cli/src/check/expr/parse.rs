use super::ast::*;
use super::atom::{build_atom, parse_atom, parse_op, parse_rhs, unquote};
use super::collection::try_parse_collection_subset_atom;
use super::cursor::{Parser, operator_at};
use super::domain::parse_domain_filter_body;
use super::error::ParseError;
use super::number::{next_starts_number_call, parse_number_expr};
use super::value::parse_mode_lhs;

pub(in crate::check) fn parse(
	input: &str,
	scheme: &str,
	allowed_kinds: &[&str],
) -> Result<Expr, ParseError> {
	let raw = input.to_string();
	let mut p = Parser::new(input, scheme, allowed_kinds, &raw);
	let root = parse_expr(&mut p)?;
	p.skip_ws();
	if p.pos < p.input.len() {
		let msg = format!("trailing input at byte {}: `{}`", p.pos, &p.input[p.pos..]);
		return Err(ParseError::BadExpr { expr: raw, msg });
	}
	Ok(Expr { root })
}

pub(super) fn parse_expr(p: &mut Parser<'_>) -> Result<Node, ParseError> {
	let lhs = parse_or(p)?;
	p.skip_ws();
	if p.eat_keyword("=>") {
		let rhs = parse_or(p)?;
		return Ok(Node::Implies(Box::new(lhs), Box::new(rhs)));
	}
	Ok(lhs)
}

fn parse_or(p: &mut Parser<'_>) -> Result<Node, ParseError> {
	let mut nodes = vec![parse_and(p)?];
	loop {
		p.skip_ws();
		if !p.eat_keyword("OR") {
			break;
		}
		nodes.push(parse_and(p)?);
	}
	Ok(if nodes.len() == 1 {
		nodes.pop().unwrap()
	} else {
		Node::Or(nodes)
	})
}

fn parse_and(p: &mut Parser<'_>) -> Result<Node, ParseError> {
	let mut nodes = vec![parse_not(p)?];
	loop {
		p.skip_ws();
		if !p.eat_keyword("AND") {
			break;
		}
		nodes.push(parse_not(p)?);
	}
	Ok(if nodes.len() == 1 {
		nodes.pop().unwrap()
	} else {
		Node::And(nodes)
	})
}

fn parse_not(p: &mut Parser<'_>) -> Result<Node, ParseError> {
	p.skip_ws();
	if p.eat_keyword("NOT") {
		let inner = parse_not(p)?;
		return Ok(Node::Not(Box::new(inner)));
	}
	parse_primary(p)
}

fn parse_primary(p: &mut Parser<'_>) -> Result<Node, ParseError> {
	p.skip_ws();
	if p.peek_byte() == Some(b'(') {
		p.pos += 1;
		let inner = parse_expr(p)?;
		p.skip_ws();
		if p.peek_byte() != Some(b')') {
			return Err(ParseError::BadExpr {
				expr: p.raw.to_string(),
				msg: format!("missing `)` at byte {}", p.pos),
			});
		}
		p.pos += 1;
		return Ok(inner);
	}
	if let Some(q) = try_parse_quantifier(p)? {
		return Ok(q);
	}
	if let Some(atom) = try_parse_collection_subset_atom(p)? {
		return Ok(Node::Atom(atom));
	}
	if let Some(atom) = try_parse_number_atom(p)? {
		return Ok(Node::Atom(atom));
	}
	if let Some(atom) = try_parse_mode_atom(p)? {
		return Ok(Node::Atom(atom));
	}
	if let Some(atom) = try_parse_segment_atom(p)? {
		return Ok(Node::Atom(atom));
	}
	let atom_end = p.find_atom_end();
	if atom_end == p.pos {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected atom at byte {}", p.pos),
		});
	}
	let atom_str = &p.input[p.pos..atom_end];
	let atom = parse_atom(
		atom_str,
		p.scheme,
		p.allowed_kinds,
		p.raw,
		p.pair_bindings_allowed,
	)?;
	p.pos = atom_end;
	Ok(Node::Atom(atom))
}

fn try_parse_segment_atom(p: &mut Parser<'_>) -> Result<Option<Atom>, ParseError> {
	p.skip_ws();
	let rest = &p.input[p.pos..];
	let (scope, prefix_len) = if rest.starts_with("source.segment(") {
		(SegmentScope::Source, "source.segment(".len())
	} else if rest.starts_with("target.segment(") {
		(SegmentScope::Target, "target.segment(".len())
	} else if rest.starts_with("segment(") {
		(SegmentScope::Def, "segment(".len())
	} else {
		return Ok(None);
	};
	let raw_start = p.pos;
	p.pos += prefix_len;
	let arg = p
		.take_until_byte(b')')
		.ok_or_else(|| ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: "unclosed `segment(...)` projection".to_string(),
		})?
		.trim();
	let kind = unquote(arg).to_string();
	if kind.is_empty() {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: "segment(<kind>) needs a kind argument".to_string(),
		});
	}
	p.pos += 1;
	Ok(Some(parse_comparison_tail(
		p,
		raw_start,
		LhsExpr::SegmentOf { scope, kind },
		"expected `<op> <rhs>` after `segment(...)`",
	)?))
}

fn try_parse_number_atom(p: &mut Parser<'_>) -> Result<Option<Atom>, ParseError> {
	p.skip_ws();
	if !next_starts_number_call(p) {
		return Ok(None);
	}
	let raw_start = p.pos;
	let lhs = parse_number_expr(p)?;
	Ok(Some(parse_comparison_tail(
		p,
		raw_start,
		LhsExpr::Number(lhs),
		"expected numeric comparison after number expression",
	)?))
}

fn try_parse_mode_atom(p: &mut Parser<'_>) -> Result<Option<Atom>, ParseError> {
	p.skip_ws();
	if !p.input[p.pos..].starts_with("mode(") {
		return Ok(None);
	}
	let raw_start = p.pos;
	let lhs = parse_mode_lhs(p)?;
	Ok(Some(parse_comparison_tail(
		p,
		raw_start,
		lhs,
		"expected comparison after `mode(...)`",
	)?))
}

fn try_parse_quantifier(p: &mut Parser<'_>) -> Result<Option<Node>, ParseError> {
	p.skip_ws();
	for (kw, qk) in [
		("any", QuantKind::Any),
		("all", QuantKind::All),
		("none", QuantKind::None),
	] {
		if let Some(rest) = p.input[p.pos..].strip_prefix(kw)
			&& rest.starts_with('(')
		{
			p.pos += kw.len();
			let (domain, filter) = parse_domain_filter_body(p, parse_expr)?;
			let filter = filter.ok_or_else(|| ParseError::BadExpr {
				expr: p.raw.to_string(),
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
	p: &mut Parser<'_>,
	raw_start: usize,
	lhs: LhsExpr,
	missing_op_msg: &str,
) -> Result<Atom, ParseError> {
	p.skip_ws();
	let (op_str, op_len) = operator_at(&p.input[p.pos..]).ok_or_else(|| ParseError::BadExpr {
		expr: p.raw.to_string(),
		msg: format!("{missing_op_msg} at byte {}", p.pos),
	})?;
	p.pos += op_len;
	let op = parse_op(op_str, p.raw)?;
	p.skip_ws();
	let rhs_end = p.find_atom_end();
	let rhs_str = p.input[p.pos..rhs_end].trim();
	if rhs_str.is_empty() {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: "empty RHS after comparison op".to_string(),
		});
	}
	let rhs = parse_rhs(
		rhs_str,
		op,
		p.scheme,
		p.allowed_kinds,
		p.raw,
		p.pair_bindings_allowed,
	)?;
	p.pos = rhs_end;
	let raw = p.input[raw_start..p.pos].to_string();
	build_atom(lhs, op, rhs, raw, p.raw)
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
