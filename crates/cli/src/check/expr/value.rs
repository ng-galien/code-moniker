use super::ast::*;
use super::cursor::{self, ParseResult, ParserState};
use super::domain::{parse_domain_ident, reject_pair_domain};
use super::error::ParseError;
use super::number::{next_starts_number_call, parse_number_expr};

pub(super) fn parse_mode_lhs<'a>(state: ParserState<'a>) -> ParseResult<'a, LhsExpr> {
	let state = cursor::advance(state, "mode".len());
	let (body, state) = parse_domain_value_call_body(state)?;
	Ok((LhsExpr::Mode(body), state))
}

pub(super) fn parse_domain_value_call_body<'a>(
	state: ParserState<'a>,
) -> ParseResult<'a, DomainValueExpr> {
	if cursor::peek_byte(&state) != Some(b'(') {
		return Err(cursor::bail(
			&state,
			format!("expected `(` at byte {}", cursor::position(&state)),
		));
	}
	let state = cursor::advance(state, 1);
	let state = cursor::skip_ws(state);
	let (domain, state) = parse_domain_ident(state)?;
	reject_pair_domain(&state, &domain, "domain value expressions")?;
	let state = cursor::skip_ws(state);
	let (expr, state) = if cursor::peek_byte(&state) == Some(b',') {
		parse_value_expr(cursor::advance(state, 1))?
	} else {
		parse_legacy_projection_value(state)?
	};
	let state = cursor::skip_ws(state);
	if cursor::peek_byte(&state) != Some(b')') {
		return Err(cursor::bail(
			&state,
			format!(
				"missing `)` for domain value expression at byte {}",
				cursor::position(&state)
			),
		));
	}
	Ok((
		DomainValueExpr {
			domain,
			expr: Box::new(expr),
		},
		cursor::advance(state, 1),
	))
}

fn parse_legacy_projection_value<'a>(state: ParserState<'a>) -> ParseResult<'a, ValueExpr> {
	let mut projection = Vec::new();
	let mut state = state;
	loop {
		state = cursor::skip_ws(state);
		if cursor::peek_byte(&state) != Some(b'.') {
			break;
		}
		let (segment, next_state) = cursor::take_projection_segment(cursor::advance(state, 1));
		state = next_state;
		if segment.is_empty() {
			return Err(cursor::bail(
				&state,
				format!(
					"expected projection segment at byte {}",
					cursor::position(&state)
				),
			));
		}
		projection.push(segment.to_string());
	}
	if projection.is_empty() {
		return Ok((ValueExpr::Item, state));
	}
	let raw = projection.join(".");
	let Some(lhs) = Lhs::from_projection_name(&raw) else {
		return Err(ParseError::BadExpr {
			expr: cursor::raw(&state).to_string(),
			msg: format!("unknown projection `{raw}`"),
		});
	};
	Ok((ValueExpr::Projection(lhs), state))
}

fn parse_value_expr<'a>(state: ParserState<'a>) -> ParseResult<'a, ValueExpr> {
	let state = cursor::skip_ws(state);
	if next_starts_number_call(&state)
		|| cursor::peek_byte(&state).is_some_and(|b| b.is_ascii_digit())
	{
		let (number, state) = parse_number_expr(state)?;
		return Ok((ValueExpr::Number(number), state));
	}
	let (raw, state) = cursor::take_projection_token(state);
	let Some(lhs) = Lhs::from_projection_name(raw) else {
		return Err(ParseError::BadExpr {
			expr: cursor::raw(&state).to_string(),
			msg: format!("expected value expression, got `{raw}`"),
		});
	};
	Ok((ValueExpr::Projection(lhs), state))
}
