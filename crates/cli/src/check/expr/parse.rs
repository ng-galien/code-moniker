use super::ast::*;
use super::atom::{build_atom, parse_atom, parse_op, parse_rhs, unquote};
use super::collection::try_parse_collection_subset_atom;
use super::cursor::{self, ParseResult, ParserState};
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
	let state = ParserState::new(input, scheme, allowed_kinds, &raw);
	let (root, state) = parse_expr(state)?;
	let state = cursor::skip_ws(state);
	if !cursor::is_at_end(&state) {
		let msg = format!(
			"trailing input at byte {}: `{}`",
			cursor::position(&state),
			cursor::rest(&state)
		);
		return Err(ParseError::BadExpr { expr: raw, msg });
	}
	Ok(Expr { root })
}

pub(super) fn parse_expr<'a>(state: ParserState<'a>) -> ParseResult<'a, Node> {
	let (lhs, state) = parse_or(state)?;
	let state = cursor::skip_ws(state);
	let (has_arrow, state) = cursor::eat_keyword(state, "=>");
	if has_arrow {
		let (rhs, state) = parse_or(state)?;
		return Ok((Node::Implies(Box::new(lhs), Box::new(rhs)), state));
	}
	Ok((lhs, state))
}

fn parse_or<'a>(state: ParserState<'a>) -> ParseResult<'a, Node> {
	let (first, mut state) = parse_and(state)?;
	let mut nodes = vec![first];
	loop {
		state = cursor::skip_ws(state);
		let (has_or, next_state) = cursor::eat_keyword(state, "OR");
		state = next_state;
		if !has_or {
			break;
		}
		let (node, next_state) = parse_and(state)?;
		nodes.push(node);
		state = next_state;
	}
	let root = if nodes.len() == 1 {
		nodes.pop().unwrap()
	} else {
		Node::Or(nodes)
	};
	Ok((root, state))
}

fn parse_and<'a>(state: ParserState<'a>) -> ParseResult<'a, Node> {
	let (first, mut state) = parse_not(state)?;
	let mut nodes = vec![first];
	loop {
		state = cursor::skip_ws(state);
		let (has_and, next_state) = cursor::eat_keyword(state, "AND");
		state = next_state;
		if !has_and {
			break;
		}
		let (node, next_state) = parse_not(state)?;
		nodes.push(node);
		state = next_state;
	}
	let root = if nodes.len() == 1 {
		nodes.pop().unwrap()
	} else {
		Node::And(nodes)
	};
	Ok((root, state))
}

fn parse_not<'a>(state: ParserState<'a>) -> ParseResult<'a, Node> {
	let state = cursor::skip_ws(state);
	let (has_not, state) = cursor::eat_keyword(state, "NOT");
	if has_not {
		let (inner, state) = parse_not(state)?;
		return Ok((Node::Not(Box::new(inner)), state));
	}
	parse_primary(state)
}

fn parse_primary<'a>(state: ParserState<'a>) -> ParseResult<'a, Node> {
	let state = cursor::skip_ws(state);
	if cursor::peek_byte(&state) == Some(b'(') {
		let state = cursor::advance(state, 1);
		let (inner, state) = parse_expr(state)?;
		let state = cursor::skip_ws(state);
		if cursor::peek_byte(&state) != Some(b')') {
			return Err(cursor::bail(
				&state,
				format!("missing `)` at byte {}", cursor::position(&state)),
			));
		}
		return Ok((inner, cursor::advance(state, 1)));
	}
	let (q, state) = try_parse_quantifier(state)?;
	if let Some(q) = q {
		return Ok((q, state));
	}
	let (atom, state) = try_parse_collection_subset_atom(state)?;
	if let Some(atom) = atom {
		return Ok((Node::Atom(atom), state));
	}
	let (atom, state) = try_parse_number_atom(state)?;
	if let Some(atom) = atom {
		return Ok((Node::Atom(atom), state));
	}
	let (atom, state) = try_parse_mode_atom(state)?;
	if let Some(atom) = atom {
		return Ok((Node::Atom(atom), state));
	}
	let (atom, state) = try_parse_segment_atom(state)?;
	if let Some(atom) = atom {
		return Ok((Node::Atom(atom), state));
	}
	let (atom_start, atom_str, state) = cursor::take_atom_text(state);
	if atom_str.is_empty() {
		return Err(cursor::bail(
			&state,
			format!("expected atom at byte {}", atom_start),
		));
	}
	let atom = parse_atom(
		atom_str,
		cursor::scheme(&state),
		cursor::allowed_kinds(&state),
		cursor::raw(&state),
		cursor::pair_bindings_allowed(&state),
	)?;
	Ok((Node::Atom(atom), state))
}

fn try_parse_segment_atom<'a>(state: ParserState<'a>) -> ParseResult<'a, Option<Atom>> {
	let state = cursor::skip_ws(state);
	let rest = cursor::rest(&state);
	let (scope, prefix_len) = if rest.starts_with("source.segment(") {
		(SegmentScope::Source, "source.segment(".len())
	} else if rest.starts_with("target.segment(") {
		(SegmentScope::Target, "target.segment(".len())
	} else if rest.starts_with("segment(") {
		(SegmentScope::Def, "segment(".len())
	} else {
		return Ok((None, state));
	};
	let raw_start = cursor::position(&state);
	let state = cursor::advance(state, prefix_len);
	let unclosed_state = state.clone();
	let (arg, state) = cursor::take_until_byte(state, b')')
		.ok_or_else(|| cursor::bail(&unclosed_state, "unclosed `segment(...)` projection"))?;
	let arg = arg.trim();
	let kind = unquote(arg).to_string();
	if kind.is_empty() {
		return Err(cursor::bail(
			&state,
			"segment(<kind>) needs a kind argument",
		));
	}
	let state = cursor::advance(state, 1);
	let (atom, state) = parse_comparison_tail(
		state,
		raw_start,
		LhsExpr::SegmentOf { scope, kind },
		"expected `<op> <rhs>` after `segment(...)`",
	)?;
	Ok((Some(atom), state))
}

fn try_parse_number_atom<'a>(state: ParserState<'a>) -> ParseResult<'a, Option<Atom>> {
	let state = cursor::skip_ws(state);
	if !next_starts_number_call(&state) {
		return Ok((None, state));
	}
	let raw_start = cursor::position(&state);
	let (lhs, state) = parse_number_expr(state)?;
	let (atom, state) = parse_comparison_tail(
		state,
		raw_start,
		LhsExpr::Number(lhs),
		"expected numeric comparison after number expression",
	)?;
	Ok((Some(atom), state))
}

fn try_parse_mode_atom<'a>(state: ParserState<'a>) -> ParseResult<'a, Option<Atom>> {
	let state = cursor::skip_ws(state);
	if !cursor::starts_with(&state, "mode(") {
		return Ok((None, state));
	}
	let raw_start = cursor::position(&state);
	let (lhs, state) = parse_mode_lhs(state)?;
	let (atom, state) = parse_comparison_tail(
		state,
		raw_start,
		lhs,
		"expected comparison after `mode(...)`",
	)?;
	Ok((Some(atom), state))
}

fn try_parse_quantifier<'a>(state: ParserState<'a>) -> ParseResult<'a, Option<Node>> {
	let state = cursor::skip_ws(state);
	for (kw, qk) in [
		("any", QuantKind::Any),
		("all", QuantKind::All),
		("none", QuantKind::None),
	] {
		if let Some(rest) = cursor::rest(&state).strip_prefix(kw)
			&& rest.starts_with('(')
		{
			let state = cursor::advance(state, kw.len());
			let ((domain, filter), state) = parse_domain_filter_body(state, parse_expr)?;
			let filter = filter.ok_or_else(|| {
				cursor::bail(
					&state,
					format!("`{kw}` requires a filter expression: `{kw}(<domain>, <expr>)`"),
				)
			})?;
			return Ok((
				Some(Node::Quantifier {
					kind: qk,
					domain,
					filter: Box::new(filter),
				}),
				state,
			));
		}
	}
	Ok((None, state))
}

fn parse_comparison_tail<'a>(
	state: ParserState<'a>,
	raw_start: usize,
	lhs: LhsExpr,
	missing_op_msg: &str,
) -> ParseResult<'a, Atom> {
	let state = cursor::skip_ws(state);
	let (op_str, op_len) = cursor::operator(&state).ok_or_else(|| {
		cursor::bail(
			&state,
			format!("{missing_op_msg} at byte {}", cursor::position(&state)),
		)
	})?;
	let state = cursor::advance(state, op_len);
	let op = parse_op(op_str, cursor::raw(&state))?;
	let state = cursor::skip_ws(state);
	let (_rhs_start, rhs_raw, state) = cursor::take_atom_text(state);
	let rhs_str = rhs_raw.trim();
	if rhs_str.is_empty() {
		return Err(cursor::bail(&state, "empty RHS after comparison op"));
	}
	let rhs = parse_rhs(
		rhs_str,
		op,
		cursor::scheme(&state),
		cursor::allowed_kinds(&state),
		cursor::raw(&state),
		cursor::pair_bindings_allowed(&state),
	)?;
	let raw = cursor::slice_from(&state, raw_start).to_string();
	let atom = build_atom(lhs, op, rhs, raw, cursor::raw(&state))?;
	Ok((atom, state))
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
