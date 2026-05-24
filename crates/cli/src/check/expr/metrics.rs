use super::ast::*;
use super::cursor::{self, ParseResult, ParserState};
use super::error::ParseError;

pub(super) fn starts_metric_call(state: &ParserState<'_>) -> bool {
	metric_prefix(state).is_some()
}

pub(super) fn try_parse_metric_expr<'a>(
	state: ParserState<'a>,
) -> ParseResult<'a, Option<NumberExpr>> {
	let Some((name, kind)) = metric_prefix(&state) else {
		return Ok((None, state));
	};
	let state = cursor::advance(state, name.len());
	let (binding, state) = parse_metric_call_body(state, name)?;
	Ok((Some(NumberExpr::Metric { kind, binding }), state))
}

fn metric_prefix(state: &ParserState<'_>) -> Option<(&'static str, MetricKind)> {
	let rest = cursor::rest(state);
	[
		("fan_out", MetricKind::FanOut),
		("fan_in", MetricKind::FanIn),
		("lcom4", MetricKind::Lcom4),
		("cbo", MetricKind::Cbo),
		("rfc", MetricKind::Rfc),
		("wmc", MetricKind::Wmc),
		("dit", MetricKind::Dit),
		("noc", MetricKind::Noc),
	]
	.into_iter()
	.find(|(name, _)| {
		rest.strip_prefix(*name)
			.is_some_and(|after| after.starts_with('('))
	})
}

fn parse_metric_call_body<'a>(state: ParserState<'a>, name: &str) -> ParseResult<'a, Binding> {
	if cursor::peek_byte(&state) != Some(b'(') {
		return Err(cursor::bail(&state, format!("expected `(` after `{name}`")));
	}
	let state = cursor::advance(state, 1);
	let state = cursor::skip_ws(state);
	let (raw, state) = cursor::take_alpha_token(state);
	let binding = match raw {
		"self" => Binding::Self_,
		"each" => Binding::Each,
		_ => {
			return Err(ParseError::BadExpr {
				expr: cursor::raw(&state).to_string(),
				msg: format!("unknown metric binding `{raw}` (allowed: self, each)"),
			});
		}
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
	Ok((binding, cursor::advance(state, 1)))
}

#[cfg(test)]
mod tests {
	use super::super::parse;
	use super::super::test_support::{KINDS, TS, solo};
	use super::super::*;

	#[test]
	fn parses_named_metric_comparison() {
		let e = parse("lcom4(self) <= 1", TS, KINDS).unwrap();
		let a = solo(&e);
		match &a.lhs {
			LhsExpr::Number(NumberExpr::Metric {
				kind: MetricKind::Lcom4,
				binding: Binding::Self_,
			}) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_fan_out_as_metric_expression() {
		let e = parse("cv(method, fan_out(each)) <= 0.6", TS, KINDS).unwrap();
		let a = solo(&e);
		match &a.lhs {
			LhsExpr::Number(NumberExpr::Aggregate { domain, expr, .. })
				if matches!(domain, Domain::Children(kind) if kind == "method")
					&& matches!(
						expr.as_ref(),
						NumberExpr::Metric {
							kind: MetricKind::FanOut,
							binding: Binding::Each,
						}
					) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}
}
