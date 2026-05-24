use super::ast::*;
use super::atom::build_atom;
use super::cursor::Parser;
use super::domain::{parse_domain_ident, reject_pair_domain};
use super::error::ParseError;

pub(super) fn try_parse_collection_subset_atom(
	p: &mut Parser<'_>,
) -> Result<Option<Atom>, ParseError> {
	p.skip_ws();
	let raw_start = p.pos;
	let atom_end = p.find_atom_end();
	let raw = &p.input[p.pos..atom_end];
	let Some((op_idx, op_len)) = find_top_level_subset(raw) else {
		return Ok(None);
	};
	let lhs_src = raw[..op_idx].trim();
	let rhs_src = raw[op_idx + op_len..].trim();
	if lhs_src.is_empty() || rhs_src.is_empty() {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: "collection subset requires `<collection> subset <collection>`".to_string(),
		});
	}
	let lhs = parse_collection_expr_full(
		lhs_src,
		p.scheme,
		p.allowed_kinds,
		p.raw,
		p.pair_bindings_allowed,
	)?;
	let rhs = parse_collection_expr_full(
		rhs_src,
		p.scheme,
		p.allowed_kinds,
		p.raw,
		p.pair_bindings_allowed,
	)?;
	p.pos = atom_end;
	Ok(Some(build_atom(
		LhsExpr::Collection(lhs),
		Op::Subset,
		Rhs::Collection(rhs),
		p.input[raw_start..p.pos].to_string(),
		p.raw,
	)?))
}

pub(super) fn parse_collection_call_body(
	p: &mut Parser<'_>,
	name: &str,
) -> Result<CollectionExpr, ParseError> {
	if p.peek_byte() != Some(b'(') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("expected `(` after `{name}`"),
		});
	}
	p.pos += 1;
	let expr = parse_collection_expr(p)?;
	p.skip_ws();
	if p.peek_byte() != Some(b')') {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: format!("missing `)` for `{name}` at byte {}", p.pos),
		});
	}
	p.pos += 1;
	Ok(expr)
}

fn parse_collection_expr(p: &mut Parser<'_>) -> Result<CollectionExpr, ParseError> {
	let mut expr = parse_collection_primary(p)?;
	loop {
		p.skip_ws();
		let Some(op) = eat_collection_op(p) else {
			break;
		};
		let right = parse_collection_primary(p)?;
		expr = CollectionExpr::Binary {
			op,
			left: Box::new(expr),
			right: Box::new(right),
		};
	}
	Ok(expr)
}

fn parse_collection_primary(p: &mut Parser<'_>) -> Result<CollectionExpr, ParseError> {
	p.skip_ws();
	if p.input[p.pos..].starts_with("unique(") {
		p.pos += "unique".len();
		return parse_collection_call_body(p, "unique")
			.map(|expr| CollectionExpr::Unique(Box::new(expr)));
	}
	if p.peek_byte() == Some(b'(') {
		p.pos += 1;
		let expr = parse_collection_expr(p)?;
		p.skip_ws();
		if p.peek_byte() != Some(b')') {
			return Err(ParseError::BadExpr {
				expr: p.raw.to_string(),
				msg: format!("missing `)` in collection expression at byte {}", p.pos),
			});
		}
		p.pos += 1;
		return Ok(expr);
	}
	if let Some(expr) = try_parse_pair_collection_projection(p)? {
		return Ok(expr);
	}
	let domain = parse_domain_ident(p)?;
	reject_pair_domain(p, &domain, "collection projections")?;
	let mut path = Vec::new();
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
				msg: format!("expected collection projection segment at byte {}", p.pos),
			});
		}
		path.push(segment.to_string());
	}
	validate_collection_projection_path(&domain, &path, p.raw)?;
	Ok(CollectionExpr::Projection(CollectionProjection {
		domain,
		path,
	}))
}

fn try_parse_pair_collection_projection(
	p: &mut Parser<'_>,
) -> Result<Option<CollectionExpr>, ParseError> {
	let side = if p.input[p.pos..].starts_with("a.") {
		PairSide::A
	} else if p.input[p.pos..].starts_with("b.") {
		PairSide::B
	} else {
		return Ok(None);
	};
	if !p.pair_bindings_allowed {
		return Err(ParseError::BadExpr {
			expr: p.raw.to_string(),
			msg: "pair-bound collection projections are only valid inside `pairs(...)` filters"
				.to_string(),
		});
	}
	p.pos += 2;
	let domain = parse_domain_ident(p)?;
	reject_pair_domain(p, &domain, "pair collection projections")?;
	let mut path = Vec::new();
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
				msg: format!(
					"expected pair collection projection segment at byte {}",
					p.pos
				),
			});
		}
		path.push(segment.to_string());
	}
	validate_collection_projection_path(&domain, &path, p.raw)?;
	Ok(Some(CollectionExpr::PairProjection(
		PairCollectionProjection { side, domain, path },
	)))
}

fn eat_collection_op(p: &mut Parser<'_>) -> Option<CollectionOp> {
	if p.eat_keyword("intersect") {
		Some(CollectionOp::Intersect)
	} else if p.eat_keyword("union") {
		Some(CollectionOp::Union)
	} else if p.eat_keyword("diff") {
		Some(CollectionOp::Difference)
	} else {
		None
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
	let mut p = Parser::new(s, scheme, allowed_kinds, full);
	p.pair_bindings_allowed = pair_bindings_allowed;
	let expr = parse_collection_expr(&mut p)?;
	p.skip_ws();
	if p.pos < p.input.len() {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!(
				"trailing input in collection expression `{}`",
				&p.input[p.pos..]
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
