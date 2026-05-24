use super::ast::*;
use super::atom::build_atom;
use super::cursor::{self, ParseResult, ParserState};
use super::domain::{parse_domain_ident, reject_pair_domain};
use super::error::ParseError;

pub(super) fn try_parse_collection_subset_atom<'a>(
	state: ParserState<'a>,
) -> ParseResult<'a, Option<Atom>> {
	let state = cursor::skip_ws(state);
	let (raw_start, raw) = cursor::peek_atom_text(&state);
	let Some((op_idx, op_len)) = find_top_level_subset(raw) else {
		return Ok((None, state));
	};
	let lhs_src = raw[..op_idx].trim();
	let rhs_src = raw[op_idx + op_len..].trim();
	if lhs_src.is_empty() || rhs_src.is_empty() {
		return Err(cursor::bail(
			&state,
			"collection subset requires `<collection> subset <collection>`",
		));
	}
	let lhs = parse_collection_expr_full(
		lhs_src,
		cursor::scheme(&state),
		cursor::allowed_kinds(&state),
		cursor::raw(&state),
		cursor::pair_bindings_allowed(&state),
	)?;
	let rhs = parse_collection_expr_full(
		rhs_src,
		cursor::scheme(&state),
		cursor::allowed_kinds(&state),
		cursor::raw(&state),
		cursor::pair_bindings_allowed(&state),
	)?;
	let (_raw_start, _raw, atom_state) = cursor::take_atom_text(state);
	let atom = build_atom(
		LhsExpr::Collection(lhs),
		Op::Subset,
		Rhs::Collection(rhs),
		cursor::slice_from(&atom_state, raw_start).to_string(),
		cursor::raw(&atom_state),
	)?;
	Ok((Some(atom), atom_state))
}

pub(super) fn parse_collection_call_body<'a>(
	state: ParserState<'a>,
	name: &str,
) -> ParseResult<'a, CollectionExpr> {
	if cursor::peek_byte(&state) != Some(b'(') {
		return Err(cursor::bail(&state, format!("expected `(` after `{name}`")));
	}
	let state = cursor::advance(state, 1);
	let (expr, state) = parse_collection_expr(state)?;
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
	Ok((expr, cursor::advance(state, 1)))
}

fn parse_collection_expr<'a>(state: ParserState<'a>) -> ParseResult<'a, CollectionExpr> {
	let (mut expr, mut state) = parse_collection_primary(state)?;
	loop {
		state = cursor::skip_ws(state);
		let (op, next_state) = eat_collection_op(state);
		state = next_state;
		let Some(op) = op else {
			break;
		};
		let (right, next_state) = parse_collection_primary(state)?;
		state = next_state;
		expr = CollectionExpr::Binary {
			op,
			left: Box::new(expr),
			right: Box::new(right),
		};
	}
	Ok((expr, state))
}

fn parse_collection_primary<'a>(state: ParserState<'a>) -> ParseResult<'a, CollectionExpr> {
	let state = cursor::skip_ws(state);
	if cursor::starts_with(&state, "unique(") {
		let state = cursor::advance(state, "unique".len());
		let (expr, state) = parse_collection_call_body(state, "unique")?;
		return Ok((CollectionExpr::Unique(Box::new(expr)), state));
	}
	if cursor::peek_byte(&state) == Some(b'(') {
		let state = cursor::advance(state, 1);
		let (expr, state) = parse_collection_expr(state)?;
		let state = cursor::skip_ws(state);
		if cursor::peek_byte(&state) != Some(b')') {
			return Err(cursor::bail(
				&state,
				format!(
					"missing `)` in collection expression at byte {}",
					cursor::position(&state)
				),
			));
		}
		return Ok((expr, cursor::advance(state, 1)));
	}
	let (expr, state) = try_parse_pair_collection_projection(state)?;
	if let Some(expr) = expr {
		return Ok((expr, state));
	}
	let (domain, mut state) = parse_domain_ident(state)?;
	reject_pair_domain(&state, &domain, "collection projections")?;
	let mut path = Vec::new();
	loop {
		state = cursor::skip_ws(state);
		if cursor::peek_byte(&state) != Some(b'.') {
			break;
		}
		let (segment, next_state) = cursor::take_projection_segment(cursor::advance(state, 1));
		state = next_state;
		if segment.is_empty() {
			return Err(cursor::bail(
				&state,
				format!(
					"expected collection projection segment at byte {}",
					cursor::position(&state)
				),
			));
		}
		path.push(segment.to_string());
	}
	validate_collection_projection_path(&domain, &path, cursor::raw(&state))?;
	Ok((
		CollectionExpr::Projection(CollectionProjection { domain, path }),
		state,
	))
}

fn try_parse_pair_collection_projection<'a>(
	state: ParserState<'a>,
) -> ParseResult<'a, Option<CollectionExpr>> {
	let side = if cursor::starts_with(&state, "a.") {
		PairSide::A
	} else if cursor::starts_with(&state, "b.") {
		PairSide::B
	} else {
		return Ok((None, state));
	};
	if !cursor::pair_bindings_allowed(&state) {
		return Err(cursor::bail(
			&state,
			"pair-bound collection projections are only valid inside `pairs(...)` filters",
		));
	}
	let state = cursor::advance(state, 2);
	let (domain, mut state) = parse_domain_ident(state)?;
	reject_pair_domain(&state, &domain, "pair collection projections")?;
	let mut path = Vec::new();
	loop {
		state = cursor::skip_ws(state);
		if cursor::peek_byte(&state) != Some(b'.') {
			break;
		}
		let (segment, next_state) = cursor::take_projection_segment(cursor::advance(state, 1));
		state = next_state;
		if segment.is_empty() {
			return Err(cursor::bail(
				&state,
				format!(
					"expected pair collection projection segment at byte {}",
					cursor::position(&state)
				),
			));
		}
		path.push(segment.to_string());
	}
	validate_collection_projection_path(&domain, &path, cursor::raw(&state))?;
	Ok((
		Some(CollectionExpr::PairProjection(PairCollectionProjection {
			side,
			domain,
			path,
		})),
		state,
	))
}

fn eat_collection_op<'a>(state: ParserState<'a>) -> (Option<CollectionOp>, ParserState<'a>) {
	let (matched, state) = cursor::eat_keyword(state, "intersect");
	if matched {
		(Some(CollectionOp::Intersect), state)
	} else {
		let (matched, state) = cursor::eat_keyword(state, "union");
		if matched {
			(Some(CollectionOp::Union), state)
		} else {
			let (matched, state) = cursor::eat_keyword(state, "diff");
			if matched {
				(Some(CollectionOp::Difference), state)
			} else {
				(None, state)
			}
		}
	}
}

fn validate_collection_projection_path(
	domain: &Domain,
	path: &[String],
	full: &str,
) -> Result<(), ParseError> {
	let valid = match domain {
		Domain::Children(_) | Domain::ChildrenByShape(_) => valid_def_collection_path(path),
		Domain::OutRefs | Domain::InRefs => valid_ref_collection_path(path),
		Domain::Segments => valid_segment_collection_path(path),
		Domain::Pairs(_) => false,
	};
	if valid {
		Ok(())
	} else {
		Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!("unknown collection projection `{}`", path.join(".")),
		})
	}
}

fn valid_def_collection_path(path: &[String]) -> bool {
	match path {
		[] => true,
		[one] => matches!(
			one.as_str(),
			"self" | "name" | "kind" | "shape" | "visibility" | "lines" | "depth" | "parent"
		),
		[parent, child] if parent == "parent" => {
			matches!(child.as_str(), "name" | "kind" | "shape")
		}
		[nested, rest @ ..] if nested == "out_refs" || nested == "in_refs" => {
			valid_ref_collection_path(rest)
		}
		_ => false,
	}
}

fn valid_ref_collection_path(path: &[String]) -> bool {
	match path {
		[] => true,
		[one] => matches!(one.as_str(), "kind" | "source" | "target"),
		[side, projection] if side == "source" || side == "target" => {
			matches!(
				projection.as_str(),
				"name" | "kind" | "shape" | "visibility" | "parent"
			)
		}
		_ => false,
	}
}

fn valid_segment_collection_path(path: &[String]) -> bool {
	matches!(path, [one] if one == "kind" || one == "name")
}

pub(super) fn parse_collection_rhs(
	s: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	full: &str,
	pair_bindings_allowed: bool,
) -> Result<CollectionExpr, ParseError> {
	parse_collection_expr_full(s, scheme, allowed_kinds, full, pair_bindings_allowed)
}

fn parse_collection_expr_full(
	s: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	full: &str,
	pair_bindings_allowed: bool,
) -> Result<CollectionExpr, ParseError> {
	let state = cursor::with_pair_bindings_allowed(
		ParserState::new(s, scheme, allowed_kinds, full),
		pair_bindings_allowed,
	);
	let (expr, state) = parse_collection_expr(state)?;
	let state = cursor::skip_ws(state);
	if !cursor::is_at_end(&state) {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!(
				"trailing input in collection expression `{}`",
				cursor::rest(&state)
			),
		});
	}
	Ok(expr)
}

fn find_top_level_subset(input: &str) -> Option<(usize, usize)> {
	let mut depth: i32 = 0;
	let mut in_string: Option<char> = None;
	for (idx, ch) in input.char_indices() {
		if let Some(q) = in_string {
			if ch == q {
				in_string = None;
			}
			continue;
		}
		match ch {
			'\'' | '"' => in_string = Some(ch),
			'(' => depth += 1,
			')' => depth -= 1,
			_ if depth == 0 && keyword_at(input, idx, "subset") => {
				return Some((idx, "subset".len()));
			}
			_ => {}
		}
	}
	None
}

fn keyword_at(input: &str, idx: usize, keyword: &str) -> bool {
	if !input[idx..].starts_with(keyword) {
		return false;
	}
	let before = &input[..idx];
	let after = &input[idx + keyword.len()..];
	let before_ok = before
		.chars()
		.next_back()
		.is_some_and(|ch| ch.is_ascii_whitespace());
	let after_ok = after
		.chars()
		.next()
		.is_some_and(|ch| ch.is_ascii_whitespace() || ch == '(');
	before_ok && after_ok
}

#[cfg(test)]
mod tests {
	use super::super::parse;
	use super::super::test_support::{KINDS, TS, solo};
	use super::super::*;

	#[test]
	fn parses_size_unique_collection() {
		let e = parse("size(unique(method.name)) = size(method.name)", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.rhs) {
			(
				LhsExpr::Number(NumberExpr::Size(CollectionExpr::Unique(_))),
				Rhs::Number(NumberExpr::Size(CollectionExpr::Projection(CollectionProjection {
					domain: Domain::Children(kind),
					path,
				}))),
			) if kind == "method" && path == &["name"] => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_collection_subset() {
		let e = parse("method.name subset unique(method.name)", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(
				LhsExpr::Collection(CollectionExpr::Projection(CollectionProjection {
					domain: Domain::Children(kind),
					path,
				})),
				Op::Subset,
				Rhs::Collection(CollectionExpr::Unique(_)),
			) if kind == "method" && path == &["name"] => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_collection_binary_ops() {
		let e = parse(
			"size(unique(field.in_refs.source.parent) diff unique(method.parent)) = 0",
			TS,
			KINDS,
		)
		.unwrap();
		let a = solo(&e);
		match &a.lhs {
			LhsExpr::Number(NumberExpr::Size(CollectionExpr::Binary {
				op: CollectionOp::Difference,
				..
			})) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_intersection_and_union() {
		let e = parse(
			"size(method.name intersect unique(method.name) union field.name) >= 1",
			TS,
			KINDS,
		)
		.unwrap();
		let a = solo(&e);
		match &a.lhs {
			LhsExpr::Number(NumberExpr::Size(CollectionExpr::Binary {
				op: CollectionOp::Union,
				..
			})) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn rejects_unknown_collection_projection() {
		let r = parse("field.nmae subset method.name", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn rejects_pair_domain_as_collection_projection() {
		let r = parse("size(pairs(method)) = 0", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn parses_pair_bound_collection_projection() {
		let e = parse(
			"count(pairs(method), size(a.param.name intersect b.param.name) >= 3) = 0",
			TS,
			KINDS,
		)
		.unwrap();
		let a = solo(&e);
		match &a.lhs {
			LhsExpr::Number(NumberExpr::Count {
				domain: Domain::Pairs(inner),
				filter: Some(filter),
			}) if matches!(inner.as_ref(), Domain::Children(kind) if kind == "method") => {
				match filter.as_ref() {
					Node::Atom(Atom {
						lhs:
							LhsExpr::Number(NumberExpr::Size(CollectionExpr::Binary {
								op: CollectionOp::Intersect,
								left,
								right,
							})),
						..
					}) => {
						assert!(matches!(
							left.as_ref(),
							CollectionExpr::PairProjection(PairCollectionProjection {
								side: PairSide::A,
								domain: Domain::Children(kind),
								path,
							}) if kind == "param" && path == &["name"]
						));
						assert!(matches!(
							right.as_ref(),
							CollectionExpr::PairProjection(PairCollectionProjection {
								side: PairSide::B,
								domain: Domain::Children(kind),
								path,
							}) if kind == "param" && path == &["name"]
						));
					}
					other => panic!("unexpected filter: {other:?}"),
				}
			}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn rejects_pair_bound_collection_projection_outside_pairs_filter() {
		let r = parse("size(a.param.name) >= 1", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn parses_pair_bound_collection_projection_on_numeric_rhs() {
		let e = parse(
			"count(pairs(method), lines <= size(a.param.name intersect b.param.name)) = 0",
			TS,
			KINDS,
		)
		.unwrap();
		let a = solo(&e);
		assert!(matches!(
			&a.lhs,
			LhsExpr::Number(NumberExpr::Count {
				domain: Domain::Pairs(_),
				filter: Some(_),
			})
		));
	}
}
