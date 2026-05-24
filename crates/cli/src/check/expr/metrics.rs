use super::ast::*;
use super::cursor::Parser;
use super::error::ParseError;

pub(super) fn starts_metric_call(p: &Parser<'_>) -> bool {
	metric_prefix(p).is_some()
}

pub(super) fn try_parse_metric_expr(p: &mut Parser<'_>) -> Result<Option<NumberExpr>, ParseError> {
	let Some((name, kind)) = metric_prefix(p) else {
		return Ok(None);
	};
	p.pos += name.len();
	let binding = parse_metric_call_body(p, name)?;
	Ok(Some(NumberExpr::Metric { kind, binding }))
}

fn metric_prefix(p: &Parser<'_>) -> Option<(&'static str, MetricKind)> {
	let rest = &p.input[p.pos..];
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

fn parse_metric_call_body(p: &mut Parser<'_>, name: &str) -> Result<Binding, ParseError> {
	if p.peek_byte() != Some(b'(') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected `(` after `{name}`"),
		});
	}
	p.pos += 1;
	p.skip_ws();
	let raw = p.take_alpha_token();
	let binding = match raw {
		"self" => Binding::Self_,
		"each" => Binding::Each,
		_ => {
			return Err(ParseError::BadExpr {
				expr: p.raw.to_string(),
				msg: format!("unknown metric binding `{raw}` (allowed: self, each)"),
			});
		}
	};
	p.skip_ws();
	if p.peek_byte() != Some(b')') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("missing `)` for `{name}` at byte {}", p.pos),
		});
	}
	p.pos += 1;
	Ok(binding)
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
