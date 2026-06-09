use super::ast::*;
use super::cursor::{self, ParseResult, ParserState};
use super::domain::parse_domain_ident;
use super::error::ParseError;

pub(super) fn parse_pair_domain<'a>(state: ParserState<'a>) -> ParseResult<'a, Domain> {
	if !cursor::starts_with(&state, "pairs(") {
		return Err(cursor::bail(
			&state,
			format!("expected `pairs(` at byte {}", cursor::position(&state)),
		));
	}
	let state = cursor::advance(state, "pairs(".len());
	let state = cursor::skip_ws(state);
	let (inner, state) = parse_domain_ident(state)?;
	if matches!(inner, Domain::Pairs(_)) {
		return Err(ParseError::BadExpr {
			expr: cursor::raw(&state).to_string(),
			msg: "nested `pairs(...)` domains are not supported".to_string(),
		});
	}
	let state = cursor::skip_ws(state);
	if cursor::peek_byte(&state) != Some(b')') {
		return Err(cursor::bail(
			&state,
			format!(
				"missing `)` for `pairs(...)` at byte {}",
				cursor::position(&state)
			),
		));
	}
	Ok((Domain::Pairs(Box::new(inner)), cursor::advance(state, 1)))
}

pub(super) fn parse_pair_projection(
	s: &str,
	full: &str,
	pair_bindings_allowed: bool,
) -> Result<Option<PairProjection>, ParseError> {
	let s = s.trim();
	let (side_raw, projection_raw) = match s.split_once('.') {
		Some((side, projection)) => (side, projection),
		None => (s, "self"),
	};
	let side = match side_raw {
		"a" => PairSide::A,
		"b" => PairSide::B,
		_ => return Ok(None),
	};
	if !pair_bindings_allowed {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!("pair binding `{side_raw}` is only valid inside `pairs(...)` filters"),
		});
	}
	if projection_raw.is_empty() {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!("pair binding `{s}` needs a projection after `.`"),
		});
	}
	let lhs = pair_projection_lhs(projection_raw).ok_or_else(|| ParseError::BadExpr {
		expr: full.to_string(),
		msg: format!("unknown pair binding projection `{s}`"),
	})?;
	Ok(Some(PairProjection { side, lhs }))
}

fn pair_projection_lhs(projection: &str) -> Option<Lhs> {
	if projection == "self" {
		Some(Lhs::Moniker)
	} else {
		Lhs::from_projection_name(projection)
	}
}
