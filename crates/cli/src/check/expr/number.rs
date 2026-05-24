use super::ast::*;
use super::collection::parse_collection_call_body;
use super::cursor::Parser;
use super::domain::{parse_domain_ident, reject_pair_domain, try_parse_count_expr};
use super::error::ParseError;
use super::metrics::{starts_metric_call, try_parse_metric_expr};
use super::value::parse_domain_value_call_body;

pub(super) fn next_starts_number_call(p: &Parser<'_>) -> bool {
	let rest = &p.input[p.pos..];
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
		|| starts_metric_call(p)
}

pub(super) fn parse_number_expr(p: &mut Parser<'_>) -> Result<NumberExpr, ParseError> {
	p.skip_ws();
	if let Some(expr) = try_parse_count_expr(p)? {
		return Ok(expr);
	}
	if let Some(expr) = try_parse_aggregate_expr(p)? {
		return Ok(expr);
	}
	if let Some(expr) = try_parse_metric_expr(p)? {
		return Ok(expr);
	}
	if p.input[p.pos..].starts_with("entropy(") {
		p.pos += "entropy".len();
		return Ok(NumberExpr::Entropy(parse_domain_value_call_body(p)?));
	}
	if p.input[p.pos..].starts_with("size(") {
		p.pos += "size".len();
		return Ok(NumberExpr::Size(parse_collection_call_body(p, "size")?));
	}

	let raw = p.take_number_literal();
	if !raw.is_empty() {
		let n = raw.parse::<f64>().map_err(|e| ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected number, got `{raw}`: {e}"),
		})?;
		return Ok(NumberExpr::Literal(n));
	}

	let raw = p.take_projection_token();
	let Some(lhs) = Lhs::from_projection_name(raw) else {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected number expression, got `{raw}`"),
		});
	};
	if !lhs.is_number_projection() {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("projection `{raw}` is not numeric"),
		});
	}
	Ok(NumberExpr::Projection(lhs))
}

fn try_parse_aggregate_expr(p: &mut Parser<'_>) -> Result<Option<NumberExpr>, ParseError> {
	let Some((name, kind)) = aggregate_prefix(p) else {
		return Ok(None);
	};
	p.pos += name.len();
	if p.peek_byte() != Some(b'(') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected `(` after `{name}`"),
		});
	}
	p.pos += 1;
	p.skip_ws();
	let domain = parse_domain_ident(p)?;
	reject_pair_domain(p, &domain, name)?;
	p.skip_ws();
	if p.peek_byte() != Some(b',') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("`{name}` requires `<domain>, <expr>`"),
		});
	}
	p.pos += 1;
	let expr = parse_number_expr(p)?;
	p.skip_ws();
	let percentile = if kind == AggregateKind::Percentile {
		if p.peek_byte() != Some(b',') {
			return Err(ParseError::BadExpr {
				expr: p.raw.to_string(),
				msg: "percentile requires a third numeric argument".to_string(),
			});
		}
		p.pos += 1;
		p.skip_ws();
		Some(parse_number_literal(p)?)
	} else {
		None
	};
	p.skip_ws();
	if p.peek_byte() != Some(b')') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("missing `)` for `{name}` at byte {}", p.pos),
		});
	}
	p.pos += 1;
	Ok(Some(NumberExpr::Aggregate {
		kind,
		domain,
		expr: Box::new(expr),
		percentile,
	}))
}

fn aggregate_prefix(p: &Parser<'_>) -> Option<(&'static str, AggregateKind)> {
	let rest = &p.input[p.pos..];
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

fn parse_number_literal(p: &mut Parser<'_>) -> Result<f64, ParseError> {
	let start = p.pos;
	let raw = p.take_number_literal();
	if raw.is_empty() {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected number at byte {}", start),
		});
	}
	raw.parse::<f64>().map_err(|e| ParseError::BadExpr {
		expr: p.raw.to_string(),
		msg: format!("expected number, got `{raw}`: {e}"),
	})
}

pub(super) fn parse_number_rhs(
	s: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	full: &str,
	pair_bindings_allowed: bool,
) -> Result<NumberExpr, ParseError> {
	let mut p = Parser::new(s, scheme, allowed_kinds, full);
	p.pair_bindings_allowed = pair_bindings_allowed;
	let expr = parse_number_expr(&mut p)?;
	p.skip_ws();
	if p.pos < p.input.len() {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!(
				"trailing input in number expression `{}`",
				&p.input[p.pos..]
			),
		});
	}
	Ok(expr)
}

#[cfg(test)]
mod tests {
	use super::super::parse;
	use super::super::test_support::{KINDS, TS, solo};
	use super::super::*;

	#[test]
	fn parses_lines_le() {
		let e = parse("lines <= 60", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(
				LhsExpr::Number(NumberExpr::Projection(Lhs::Lines)),
				Op::Le,
				Rhs::Number(NumberExpr::Literal(60.0)),
			) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_count() {
		let e = parse("count(method) <= 20", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(
				LhsExpr::Number(NumberExpr::Count {
					domain: Domain::Children(k),
					filter: None,
				}),
				Op::Le,
				Rhs::Number(NumberExpr::Literal(20.0)),
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
				LhsExpr::Number(NumberExpr::Count {
					domain: Domain::Children(k),
					filter: Some(_),
				}),
				Op::Le,
			) if k == "method" => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_count_by_shape_domain() {
		let e = parse("count(shape:callable, lines <= 20) <= 5", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op) {
			(
				LhsExpr::Number(NumberExpr::Count {
					domain: Domain::ChildrenByShape(shape),
					filter: Some(_),
				}),
				Op::Le,
			) if shape == "callable" => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn rejects_unknown_shape_domain() {
		let r = parse("count(shape:ref) <= 1", TS, KINDS);
		match r {
			Err(ParseError::BadExpr { msg, .. }) => {
				assert!(msg.contains("unknown shape domain"), "{msg}");
			}
			other => panic!("expected BadExpr, got {other:?}"),
		}
	}

	#[test]
	fn parses_numeric_projection_rhs_for_ordering() {
		let e = parse("lines <= depth", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(
				LhsExpr::Number(NumberExpr::Projection(Lhs::Lines)),
				Op::Le,
				Rhs::Number(NumberExpr::Projection(Lhs::Depth)),
			) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_count_rhs_for_ordering() {
		let e = parse("count(method) <= count(function)", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(
				LhsExpr::Number(NumberExpr::Count {
					domain: Domain::Children(left),
					filter: None,
				}),
				Op::Le,
				Rhs::Number(NumberExpr::Count {
					domain: Domain::Children(right),
					filter: None,
				}),
			) if left == "method" && right == "function" => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_cv_over_domain_fan_out() {
		let e = parse("cv(method, fan_out(each)) <= 0.6", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(
				LhsExpr::Number(NumberExpr::Aggregate {
					kind: AggregateKind::Cv,
					domain: Domain::Children(kind),
					expr,
					percentile: None,
				}),
				Op::Le,
				Rhs::Number(NumberExpr::Literal(0.6)),
			) if kind == "method"
				&& matches!(
					**expr,
					NumberExpr::Metric {
						kind: MetricKind::FanOut,
						binding: Binding::Each
					}
				) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn rejects_pair_domain_in_numeric_aggregate() {
		let r = parse("sum(pairs(method), lines) = 0", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn parses_mode_projection_comparison() {
		let e = parse("mode(out_refs.target.parent) = source.parent", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(
				LhsExpr::Mode(DomainValueExpr {
					domain: Domain::OutRefs,
					expr,
				}),
				Op::Eq,
				Rhs::Projection(Lhs::SourceParentMoniker),
			) if matches!(
				expr.as_ref(),
				ValueExpr::Projection(Lhs::TargetParentMoniker)
			) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_entropy_inside_average() {
		let e = parse("avg(field, entropy(in_refs.source)) >= 0.5", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(
				LhsExpr::Number(NumberExpr::Aggregate {
					kind: AggregateKind::Avg,
					domain: Domain::Children(kind),
					expr,
					..
				}),
				Op::Ge,
				Rhs::Number(NumberExpr::Literal(0.5)),
			) if kind == "field" => match expr.as_ref() {
				NumberExpr::Entropy(DomainValueExpr {
					domain: Domain::InRefs,
					expr,
				}) if matches!(expr.as_ref(), ValueExpr::Projection(Lhs::SourceMoniker)) => {}
				other => panic!("unexpected aggregate expr: {other:?}"),
			},
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_gini_over_count_with_ref_filter() {
		let e = parse(
			"gini(field, count(in_refs, source.parent = target.parent)) <= 0.7",
			TS,
			KINDS,
		)
		.unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(
				LhsExpr::Number(NumberExpr::Aggregate {
					kind: AggregateKind::Gini,
					domain: Domain::Children(kind),
					expr,
					..
				}),
				Op::Le,
				Rhs::Number(NumberExpr::Literal(0.7)),
			) if kind == "field"
				&& matches!(
					**expr,
					NumberExpr::Count {
						domain: Domain::InRefs,
						filter: Some(_)
					}
				) => {}
			other => panic!("unexpected: {other:?}"),
		}
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
	fn rejects_non_numeric_for_lines() {
		let r = parse("lines <= forty", TS, KINDS);
		assert!(r.is_err());
	}
}
