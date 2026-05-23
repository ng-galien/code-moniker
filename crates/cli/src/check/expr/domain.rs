use super::ast::*;
use super::cursor::Parser;
use super::error::ParseError;

const DEF_SHAPE_NAMES: &[&str] = &["namespace", "type", "callable", "value", "annotation"];

fn is_def_shape_name(name: &str) -> bool {
	DEF_SHAPE_NAMES.contains(&name)
}

impl<'a> Parser<'a> {
	pub(super) fn try_parse_count_expr(&mut self) -> Result<Option<NumberExpr>, ParseError> {
		if !self.input[self.pos..].starts_with("count(") {
			return Ok(None);
		}
		self.pos += "count".len();
		let (domain, filter) = self.parse_domain_filter_body(Parser::parse_expr)?;
		Ok(Some(NumberExpr::Count {
			domain,
			filter: filter.map(Box::new),
		}))
	}

	pub(super) fn parse_domain_filter_body(
		&mut self,
		parse_filter: impl FnOnce(&mut Self) -> Result<Node, ParseError>,
	) -> Result<(Domain, Option<Node>), ParseError> {
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
		let filter = if self.peek_byte() == Some(b',') {
			self.pos += 1;
			let filter = parse_filter(self)?;
			self.skip_ws();
			Some(filter)
		} else {
			None
		};
		if self.peek_byte() != Some(b')') {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("missing `)` for quantifier at byte {}", self.pos),
			});
		}
		self.pos += 1;
		Ok((domain, filter))
	}

	pub(super) fn parse_domain_ident(&mut self) -> Result<Domain, ParseError> {
		if self.input[self.pos..].starts_with("pairs(") {
			return self.parse_pair_domain();
		}
		let start = self.pos;
		let domain_ident = self.take_domain_ident();
		if domain_ident.is_empty() {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("expected domain identifier at byte {}", start),
			});
		}
		Ok(match domain_ident {
			"segment" => Domain::Segments,
			"out_refs" => Domain::OutRefs,
			"in_refs" => Domain::InRefs,
			shape if shape.starts_with("shape:") => {
				let shape_name = shape.trim_start_matches("shape:");
				if !is_def_shape_name(shape_name) {
					return Err(ParseError::BadExpr {
						expr: self.raw.to_string(),
						msg: format!(
							"unknown shape domain `{shape_name}` (allowed: {})",
							DEF_SHAPE_NAMES.join(", ")
						),
					});
				}
				Domain::ChildrenByShape(shape_name.to_string())
			}
			other => {
				if !self.allowed_kinds.contains(&other) {
					return Err(ParseError::BadExpr {
						expr: self.raw.to_string(),
						msg: format!(
							"unknown domain `{other}` (allowed: segment, out_refs, in_refs, or one of {})",
							self.allowed_kinds.join(", ")
						),
					});
				}
				Domain::Children(other.to_string())
			}
		})
	}

	pub(super) fn reject_pair_domain(
		&self,
		domain: &Domain,
		context: &str,
	) -> Result<(), ParseError> {
		if matches!(domain, Domain::Pairs(_)) {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!(
					"`pairs(...)` domains are only supported by count/any/all/none, not {context}"
				),
			});
		}
		Ok(())
	}
}
