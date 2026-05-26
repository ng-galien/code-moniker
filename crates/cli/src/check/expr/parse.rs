use super::ast::*;
use super::atom::{build_atom, parse_atom, parse_op, parse_rhs, unquote};
use super::collection::try_parse_collection_subset_atom;
use super::cursor::{self, ParseResult, ParserState};
use super::domain::{parse_domain_filter_body, parse_domain_ident};
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
	let (m, state) = try_parse_match(state)?;
	if let Some(m) = m {
		return Ok((m, state));
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

fn try_parse_match<'a>(state: ParserState<'a>) -> ParseResult<'a, Option<Node>> {
	let state = cursor::skip_ws(state);
	if !cursor::starts_with(&state, "match(") {
		return Ok((None, state));
	}
	let raw_start = cursor::position(&state);
	let rest = cursor::rest(&state);
	let (args, consumed) = split_call_args(rest, "match")?;
	if args.len() != 5 {
		return Err(cursor::bail(
			&state,
			format!(
				"`match` expects 5 arguments: match(<left-domain>, <right-domain>, <left-key>, <right-key>, <right-filter>), got {}",
				args.len()
			),
		));
	}
	let left_domain = parse_domain_arg(args[0], &state)?;
	let right_domain = parse_domain_arg(args[1], &state)?;
	let left_key = parse_match_key(args[2].trim(), &state)?;
	let right_key = parse_match_key(args[3].trim(), &state)?;
	let filter_state = ParserState::new(
		args[4].trim(),
		cursor::scheme(&state),
		cursor::allowed_kinds(&state),
		cursor::raw(&state),
	);
	let (right_filter, filter_state) = parse_expr(filter_state)?;
	if !cursor::is_at_end(&cursor::skip_ws(filter_state)) {
		return Err(cursor::bail(
			&state,
			"`match` right filter has trailing input",
		));
	}
	let state = cursor::advance(state, consumed);
	let _raw = cursor::slice_from(&state, raw_start);
	Ok((
		Some(Node::Match {
			left_domain,
			right_domain,
			left_key,
			right_key,
			right_filter: Box::new(right_filter),
		}),
		state,
	))
}

fn parse_domain_arg(arg: &str, outer: &ParserState<'_>) -> Result<Domain, ParseError> {
	let state = ParserState::new(
		arg.trim(),
		cursor::scheme(outer),
		cursor::allowed_kinds(outer),
		cursor::raw(outer),
	);
	let (domain, state) = parse_domain_ident(state)?;
	if !cursor::is_at_end(&cursor::skip_ws(state)) {
		return Err(ParseError::BadExpr {
			expr: cursor::raw(outer).to_string(),
			msg: format!("invalid `match` domain argument `{}`", arg.trim()),
		});
	}
	Ok(domain)
}

fn parse_match_key(key: &str, state: &ParserState<'_>) -> Result<MatchKey, ParseError> {
	match key {
		"each.name" => Ok(MatchKey::EachName),
		"candidate.name" => Ok(MatchKey::CandidateName),
		"snake_case(each.name)" => Ok(MatchKey::SnakeCaseEachName),
		"module_dir(candidate.moniker)" => Ok(MatchKey::ModuleDirCandidateMoniker),
		other => Err(ParseError::BadExpr {
			expr: cursor::raw(state).to_string(),
			msg: format!(
				"unknown `match` key `{other}` (allowed: each.name, candidate.name, snake_case(each.name), module_dir(candidate.moniker))"
			),
		}),
	}
}

fn split_call_args<'a>(input: &'a str, name: &str) -> Result<(Vec<&'a str>, usize), ParseError> {
	let prefix = format!("{name}(");
	let mut args = Vec::new();
	let bytes = input.as_bytes();
	let mut depth = 0usize;
	let mut quote: Option<u8> = None;
	let mut start = prefix.len();
	let mut i = prefix.len();
	while i < bytes.len() {
		let b = bytes[i];
		if let Some(q) = quote {
			if b == b'\\' {
				i += 2;
				continue;
			}
			if b == q {
				quote = None;
			}
			i += 1;
			continue;
		}
		match b {
			b'\'' | b'"' => quote = Some(b),
			b'(' | b'[' | b'{' => depth += 1,
			b')' if depth == 0 => {
				args.push(input[start..i].trim());
				return Ok((args, i + 1));
			}
			b')' | b']' | b'}' => depth = depth.saturating_sub(1),
			b',' if depth == 0 => {
				args.push(input[start..i].trim());
				start = i + 1;
			}
			_ => {}
		}
		i += 1;
	}
	Err(ParseError::BadExpr {
		expr: input.to_string(),
		msg: format!("unclosed `{name}(...)` call"),
	})
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
