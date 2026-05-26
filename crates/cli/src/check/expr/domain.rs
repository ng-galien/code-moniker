use super::ast::*;
use super::cursor::{self, ParseResult, ParserState};
use super::error::ParseError;
use super::pairs::parse_pair_domain;
use super::parse::parse_expr;

const DEF_SHAPE_NAMES: &[&str] = &["namespace", "type", "callable", "value", "annotation"];

fn is_def_shape_name(name: &str) -> bool {
	DEF_SHAPE_NAMES.contains(&name)
}

pub(super) fn try_parse_count_expr<'a>(
	state: ParserState<'a>,
) -> ParseResult<'a, Option<NumberExpr>> {
	if !cursor::starts_with(&state, "count(") {
		return Ok((None, state));
	}
	let state = cursor::advance(state, "count".len());
	let ((domain, filter), state) = parse_domain_filter_body(state, parse_expr)?;
	Ok((
		Some(NumberExpr::Count {
			domain,
			filter: filter.map(Box::new),
		}),
		state,
	))
}

pub(super) fn parse_domain_filter_body<'a>(
	state: ParserState<'a>,
	parse_filter: impl FnOnce(ParserState<'a>) -> ParseResult<'a, Node>,
) -> ParseResult<'a, (Domain, Option<Node>)> {
	if cursor::peek_byte(&state) != Some(b'(') {
		return Err(cursor::bail(
			&state,
			format!("expected `(` at byte {}", cursor::position(&state)),
		));
	}
	let state = cursor::advance(state, 1);
	let state = cursor::skip_ws(state);
	let (domain, state) = parse_domain_ident(state)?;
	let state = cursor::skip_ws(state);
	let (filter, state) = if cursor::peek_byte(&state) == Some(b',') {
		let state = cursor::advance(state, 1);
		let previous_pair_bindings_allowed = cursor::pair_bindings_allowed(&state);
		let filter_state = cursor::with_pair_bindings_allowed(
			state,
			previous_pair_bindings_allowed || matches!(domain, Domain::Pairs(_)),
		);
		let (filter, state) = parse_filter(filter_state)?;
		let state = cursor::with_pair_bindings_allowed(state, previous_pair_bindings_allowed);
		let state = cursor::skip_ws(state);
		(Some(filter), state)
	} else {
		(None, state)
	};
	if cursor::peek_byte(&state) != Some(b')') {
		return Err(cursor::bail(
			&state,
			format!(
				"missing `)` for quantifier at byte {}",
				cursor::position(&state)
			),
		));
	}
	Ok(((domain, filter), cursor::advance(state, 1)))
}

pub(super) fn parse_domain_ident<'a>(state: ParserState<'a>) -> ParseResult<'a, Domain> {
	if cursor::starts_with(&state, "pairs(") {
		return parse_pair_domain(state);
	}
	if cursor::starts_with(&state, "project.def") {
		return Ok((
			Domain::ProjectDefs,
			cursor::advance(state, "project.def".len()),
		));
	}
	let start = cursor::position(&state);
	let (domain_ident, state) = cursor::take_domain_ident(state);
	if domain_ident.is_empty() {
		return Err(cursor::bail(
			&state,
			format!("expected domain identifier at byte {}", start),
		));
	}
	let domain = match domain_ident {
		"segment" => Domain::Segments,
		"out_refs" => Domain::OutRefs,
		"in_refs" => Domain::InRefs,
		"project.def" => Domain::ProjectDefs,
		shape if shape.starts_with("shape:") => {
			let shape_name = shape.trim_start_matches("shape:");
			if !is_def_shape_name(shape_name) {
				return Err(ParseError::BadExpr {
					expr: cursor::raw(&state).to_string(),
					msg: format!(
						"unknown shape domain `{shape_name}` (allowed: {})",
						DEF_SHAPE_NAMES.join(", ")
					),
				});
			}
			Domain::ChildrenByShape(shape_name.to_string())
		}
		other => {
			if !cursor::allowed_kinds(&state).contains(&other) {
				return Err(ParseError::BadExpr {
					expr: cursor::raw(&state).to_string(),
					msg: format!(
						"unknown domain `{other}` (allowed: segment, out_refs, in_refs, project.def, or one of {})",
						cursor::allowed_kinds(&state).join(", ")
					),
				});
			}
			Domain::Children(other.to_string())
		}
	};
	Ok((domain, state))
}

pub(super) fn reject_pair_domain(
	state: &ParserState<'_>,
	domain: &Domain,
	context: &str,
) -> Result<(), ParseError> {
	if matches!(domain, Domain::Pairs(_)) {
		return Err(ParseError::BadExpr {
			expr: cursor::raw(state).to_string(),
			msg: format!(
				"`pairs(...)` domains are only supported by count/any/all/none, not {context}"
			),
		});
	}
	Ok(())
}
