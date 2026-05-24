use super::ast::*;
use super::cursor::Parser;
use super::domain::{parse_domain_ident, reject_pair_domain};
use super::error::ParseError;
use super::number::{next_starts_number_call, parse_number_expr};

pub(super) fn parse_mode_lhs(p: &mut Parser<'_>) -> Result<LhsExpr, ParseError> {
	p.pos += "mode".len();
	Ok(LhsExpr::Mode(parse_domain_value_call_body(p)?))
}

pub(super) fn parse_domain_value_call_body(
	p: &mut Parser<'_>,
) -> Result<DomainValueExpr, ParseError> {
	if p.peek_byte() != Some(b'(') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected `(` at byte {}", p.pos),
		});
	}
	p.pos += 1;
	p.skip_ws();
	let domain = parse_domain_ident(p)?;
	reject_pair_domain(p, &domain, "domain value expressions")?;
	p.skip_ws();
	let expr = if p.peek_byte() == Some(b',') {
		p.pos += 1;
		parse_value_expr(p)?
	} else {
		parse_legacy_projection_value(p)?
	};
	p.skip_ws();
	if p.peek_byte() != Some(b')') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("missing `)` for domain value expression at byte {}", p.pos),
		});
	}
	p.pos += 1;
	Ok(DomainValueExpr {
		domain,
		expr: Box::new(expr),
	})
}

fn parse_legacy_projection_value(p: &mut Parser<'_>) -> Result<ValueExpr, ParseError> {
	let mut projection = Vec::new();
	loop {
		p.skip_ws();
		if p.peek_byte() != Some(b'.') {
			break;
		}
		p.pos += 1;
		let segment = p.take_projection_segment();
		if segment.is_empty() {
			return Err(ParseError::BadExpr {
				expr: p.raw.to_string(),
				msg: format!("expected projection segment at byte {}", p.pos),
			});
		}
		projection.push(segment.to_string());
	}
	if projection.is_empty() {
		return Ok(ValueExpr::Item);
	}
	let raw = projection.join(".");
	let Some(lhs) = Lhs::from_projection_name(&raw) else {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("unknown projection `{raw}`"),
		});
	};
	Ok(ValueExpr::Projection(lhs))
}

fn parse_value_expr(p: &mut Parser<'_>) -> Result<ValueExpr, ParseError> {
	p.skip_ws();
	if next_starts_number_call(p) || p.peek_byte().is_some_and(|b| b.is_ascii_digit()) {
		return parse_number_expr(p).map(ValueExpr::Number);
	}
	let raw = p.take_projection_token();
	let Some(lhs) = Lhs::from_projection_name(raw) else {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected value expression, got `{raw}`"),
		});
	};
	Ok(ValueExpr::Projection(lhs))
}

#[cfg(test)]
mod tests {
	use super::super::parse;
	use super::super::test_support::{KINDS, TS, solo};
	use super::super::*;

	#[test]
	fn parses_mode_with_domain_value_arguments() {
		let e = parse("mode(out_refs, target.parent) = source.parent", TS, KINDS).unwrap();
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
	fn keeps_legacy_projection_mode_syntax() {
		let e = parse("mode(out_refs.target.parent) = source.parent", TS, KINDS).unwrap();
		let a = solo(&e);
		match &a.lhs {
			LhsExpr::Mode(DomainValueExpr {
				domain: Domain::OutRefs,
				expr,
			}) if matches!(
				expr.as_ref(),
				ValueExpr::Projection(Lhs::TargetParentMoniker)
			) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_entropy_with_domain_value_arguments() {
		let e = parse("avg(field, entropy(in_refs, source)) >= 0.5", TS, KINDS).unwrap();
		let a = solo(&e);
		match &a.lhs {
			LhsExpr::Number(NumberExpr::Aggregate { expr, .. }) => match expr.as_ref() {
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
	fn rejects_pair_domain_in_domain_value_expression() {
		let r = parse("mode(pairs(method), name) = name", TS, KINDS);
		assert!(r.is_err());
	}
}
