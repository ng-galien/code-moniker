use super::ast::*;
use super::cursor::Parser;
use super::error::ParseError;
use super::pairs::parse_pair_domain;
use super::parse::parse_expr;

const DEF_SHAPE_NAMES: &[&str] = &["namespace", "type", "callable", "value", "annotation"];

fn is_def_shape_name(name: &str) -> bool {
	DEF_SHAPE_NAMES.contains(&name)
}

pub(super) fn try_parse_count_expr(p: &mut Parser<'_>) -> Result<Option<NumberExpr>, ParseError> {
	if !p.input[p.pos..].starts_with("count(") {
		return Ok(None);
	}
	p.pos += "count".len();
	let (domain, filter) = parse_domain_filter_body(p, parse_expr)?;
	Ok(Some(NumberExpr::Count {
		domain,
		filter: filter.map(Box::new),
	}))
}

pub(super) fn parse_domain_filter_body(
	p: &mut Parser<'_>,
	parse_filter: impl FnOnce(&mut Parser<'_>) -> Result<Node, ParseError>,
) -> Result<(Domain, Option<Node>), ParseError> {
	if p.peek_byte() != Some(b'(') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected `(` at byte {}", p.pos),
		});
	}
	p.pos += 1;
	p.skip_ws();
	let domain = parse_domain_ident(p)?;
	p.skip_ws();
	let filter = if p.peek_byte() == Some(b',') {
		p.pos += 1;
		let previous_pair_bindings_allowed = p.pair_bindings_allowed;
		p.pair_bindings_allowed =
			previous_pair_bindings_allowed || matches!(domain, Domain::Pairs(_));
		let filter = parse_filter(p);
		p.pair_bindings_allowed = previous_pair_bindings_allowed;
		let filter = filter?;
		p.skip_ws();
		Some(filter)
	} else {
		None
	};
	if p.peek_byte() != Some(b')') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("missing `)` for quantifier at byte {}", p.pos),
		});
	}
	p.pos += 1;
	Ok((domain, filter))
}

pub(super) fn parse_domain_ident(p: &mut Parser<'_>) -> Result<Domain, ParseError> {
	if p.input[p.pos..].starts_with("pairs(") {
		return parse_pair_domain(p);
	}
	let start = p.pos;
	let domain_ident = p.take_domain_ident();
	if domain_ident.is_empty() {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
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
					expr: p.raw.to_string(),
					msg: format!(
						"unknown shape domain `{shape_name}` (allowed: {})",
						DEF_SHAPE_NAMES.join(", ")
					),
				});
			}
			Domain::ChildrenByShape(shape_name.to_string())
		}
		other => {
			if !p.allowed_kinds.contains(&other) {
				return Err(ParseError::BadExpr {
					expr: p.raw.to_string(),
					msg: format!(
						"unknown domain `{other}` (allowed: segment, out_refs, in_refs, or one of {})",
						p.allowed_kinds.join(", ")
					),
				});
			}
			Domain::Children(other.to_string())
		}
	})
}

pub(super) fn reject_pair_domain(
	p: &Parser<'_>,
	domain: &Domain,
	context: &str,
) -> Result<(), ParseError> {
	if matches!(domain, Domain::Pairs(_)) {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!(
				"`pairs(...)` domains are only supported by count/any/all/none, not {context}"
			),
		});
	}
	Ok(())
}
