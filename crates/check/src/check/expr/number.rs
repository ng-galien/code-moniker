use super::ast::*;
use super::collection::parse_collection_call_body;
use super::cursor::{self, ParseResult, ParserState};
use super::domain::{parse_domain_ident, reject_pair_domain, try_parse_count_expr};
use super::error::ParseError;
use super::metrics::{starts_metric_call, try_parse_metric_expr};
use super::value::parse_domain_value_call_body;

pub(super) fn next_starts_number_call(state: &ParserState<'_>) -> bool {
	let rest = cursor::rest(state);
	[
		"count(",
		"sum(",
		"max(",
		"min(",
		"avg(",
		"median(",
		"percentile(",
		"stddev(",
		"var(",
		"cv(",
		"gini(",
		"entropy(",
		"size(",
	]
	.iter()
	.any(|prefix| rest.starts_with(prefix))
		|| starts_metric_call(state)
}

pub(super) fn parse_number_expr<'a>(state: ParserState<'a>) -> ParseResult<'a, NumberExpr> {
	let state = cursor::skip_ws(state);
	let (expr, state) = try_parse_count_expr(state)?;
	if let Some(expr) = expr {
		return Ok((expr, state));
	}
	let (expr, state) = try_parse_aggregate_expr(state)?;
	if let Some(expr) = expr {
		return Ok((expr, state));
	}
	let (expr, state) = try_parse_metric_expr(state)?;
	if let Some(expr) = expr {
		return Ok((expr, state));
	}
	if cursor::starts_with(&state, "entropy(") {
		let state = cursor::advance(state, "entropy".len());
		let (body, state) = parse_domain_value_call_body(state)?;
		return Ok((NumberExpr::Entropy(body), state));
	}
	if cursor::starts_with(&state, "size(") {
		let state = cursor::advance(state, "size".len());
		let (body, state) = parse_collection_call_body(state, "size")?;
		return Ok((NumberExpr::Size(body), state));
	}

	let (raw, state) = cursor::take_number_literal(state);
	if !raw.is_empty() {
		let n = raw.parse::<f64>().map_err(|e| ParseError::BadExpr {
			expr: cursor::raw(&state).to_string(),
			msg: format!("expected number, got `{raw}`: {e}"),
		})?;
		return Ok((NumberExpr::Literal(n), state));
	}

	let (raw, state) = cursor::take_projection_token(state);
	let Some(lhs) = Lhs::from_projection_name(raw) else {
		return Err(ParseError::BadExpr {
			expr: cursor::raw(&state).to_string(),
			msg: format!("expected number expression, got `{raw}`"),
		});
	};
	if !lhs.is_number_projection() {
		return Err(ParseError::BadExpr {
			expr: cursor::raw(&state).to_string(),
			msg: format!("projection `{raw}` is not numeric"),
		});
	}
	Ok((NumberExpr::Projection(lhs), state))
}

fn try_parse_aggregate_expr<'a>(state: ParserState<'a>) -> ParseResult<'a, Option<NumberExpr>> {
	let Some((name, kind)) = aggregate_prefix(&state) else {
		return Ok((None, state));
	};
	let state = cursor::advance(state, name.len());
	if cursor::peek_byte(&state) != Some(b'(') {
		return Err(cursor::bail(&state, format!("expected `(` after `{name}`")));
	}
	let state = cursor::advance(state, 1);
	let state = cursor::skip_ws(state);
	let (domain, state) = parse_domain_ident(state)?;
	reject_pair_domain(&state, &domain, name)?;
	let state = cursor::skip_ws(state);
	if cursor::peek_byte(&state) != Some(b',') {
		return Err(cursor::bail(
			&state,
			format!("`{name}` requires `<domain>, <expr>`"),
		));
	}
	let state = cursor::advance(state, 1);
	let (expr, state) = parse_number_expr(state)?;
	let state = cursor::skip_ws(state);
	let (percentile, state) = if kind == AggregateKind::Percentile {
		if cursor::peek_byte(&state) != Some(b',') {
			return Err(cursor::bail(
				&state,
				"percentile requires a third numeric argument",
			));
		}
		let state = cursor::advance(state, 1);
		let state = cursor::skip_ws(state);
		let (percentile, state) = parse_number_literal(state)?;
		(Some(percentile), state)
	} else {
		(None, state)
	};
	let state = cursor::skip_ws(state);
	if cursor::peek_byte(&state) != Some(b')') {
		return Err(cursor::bail(
			&state,
			format!(
				"missing `)` for `{name}` at byte {}",
				cursor::position(&state)
			),
		));
	}
	Ok((
		Some(NumberExpr::Aggregate {
			kind,
			domain,
			expr: Box::new(expr),
			percentile,
		}),
		cursor::advance(state, 1),
	))
}

fn aggregate_prefix(state: &ParserState<'_>) -> Option<(&'static str, AggregateKind)> {
	let rest = cursor::rest(state);
	[
		("percentile", AggregateKind::Percentile),
		("median", AggregateKind::Median),
		("stddev", AggregateKind::Stddev),
		("sum", AggregateKind::Sum),
		("max", AggregateKind::Max),
		("min", AggregateKind::Min),
		("avg", AggregateKind::Avg),
		("var", AggregateKind::Var),
		("cv", AggregateKind::Cv),
		("gini", AggregateKind::Gini),
	]
	.into_iter()
	.find(|(name, _)| rest.starts_with(&format!("{name}(")))
}

fn parse_number_literal<'a>(state: ParserState<'a>) -> ParseResult<'a, f64> {
	let start = cursor::position(&state);
	let (raw, state) = cursor::take_number_literal(state);
	if raw.is_empty() {
		return Err(cursor::bail(
			&state,
			format!("expected number at byte {}", start),
		));
	}
	let number = raw.parse::<f64>().map_err(|e| ParseError::BadExpr {
		expr: cursor::raw(&state).to_string(),
		msg: format!("expected number, got `{raw}`: {e}"),
	})?;
	Ok((number, state))
}

pub(super) fn parse_number_rhs(
	s: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	full: &str,
	pair_bindings_allowed: bool,
) -> Result<NumberExpr, ParseError> {
	let state = cursor::with_pair_bindings_allowed(
		ParserState::new(s, scheme, allowed_kinds, full),
		pair_bindings_allowed,
	);
	let (expr, state) = parse_number_expr(state)?;
	let state = cursor::skip_ws(state);
	if !cursor::is_at_end(&state) {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!(
				"trailing input in number expression `{}`",
				cursor::rest(&state)
			),
		});
	}
	Ok(expr)
}
