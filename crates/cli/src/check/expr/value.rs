use super::ast::*;
use super::cursor::Parser;
use super::error::ParseError;

impl<'a> Parser<'a> {
	pub(super) fn parse_mode_lhs(&mut self) -> Result<LhsExpr, ParseError> {
		self.pos += "mode".len();
		Ok(LhsExpr::Mode(self.parse_domain_value_call_body()?))
	}

	pub(super) fn parse_domain_value_call_body(&mut self) -> Result<DomainValueExpr, ParseError> {
		if self.peek_byte() != Some(b'(') {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("expected `(` at byte {}", self.pos),
			});
		}
		self.pos += 1;
		self.skip_ws();
		let domain = self.parse_domain_ident()?;
		self.skip_ws();
		let expr = if self.peek_byte() == Some(b',') {
			self.pos += 1;
			self.parse_value_expr()?
		} else {
			self.parse_legacy_projection_value()?
		};
		self.skip_ws();
		if self.peek_byte() != Some(b')') {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!(
					"missing `)` for domain value expression at byte {}",
					self.pos
				),
			});
		}
		self.pos += 1;
		Ok(DomainValueExpr {
			domain,
			expr: Box::new(expr),
		})
	}

	fn parse_legacy_projection_value(&mut self) -> Result<ValueExpr, ParseError> {
		let mut projection = Vec::new();
		loop {
			self.skip_ws();
			if self.peek_byte() != Some(b'.') {
				break;
			}
			self.pos += 1;
			let segment = self.take_projection_segment();
			if segment.is_empty() {
				return Err(ParseError::BadExpr {
					expr: self.raw.to_string(),
					msg: format!("expected projection segment at byte {}", self.pos),
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
				expr: self.raw.to_string(),
				msg: format!("unknown projection `{raw}`"),
			});
		};
		Ok(ValueExpr::Projection(lhs))
	}

	fn parse_value_expr(&mut self) -> Result<ValueExpr, ParseError> {
		self.skip_ws();
		if self.next_starts_number_call() || self.peek_byte().is_some_and(|b| b.is_ascii_digit()) {
			return self.parse_number_expr().map(ValueExpr::Number);
		}
		let raw = self.take_projection_token();
		let Some(lhs) = Lhs::from_projection_name(raw) else {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("expected value expression, got `{raw}`"),
			});
		};
		Ok(ValueExpr::Projection(lhs))
	}
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
}
