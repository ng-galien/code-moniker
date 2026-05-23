use super::ast::*;
use super::atom::build_atom;
use super::cursor::Parser;
use super::error::ParseError;

impl<'a> Parser<'a> {
	pub(super) fn try_parse_collection_subset_atom(&mut self) -> Result<Option<Atom>, ParseError> {
		self.skip_ws();
		let raw_start = self.pos;
		let atom_end = self.find_atom_end();
		let raw = &self.input[self.pos..atom_end];
		let Some((op_idx, op_len)) = find_top_level_subset(raw) else {
			return Ok(None);
		};
		let lhs_src = raw[..op_idx].trim();
		let rhs_src = raw[op_idx + op_len..].trim();
		if lhs_src.is_empty() || rhs_src.is_empty() {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: "collection subset requires `<collection> subset <collection>`".to_string(),
			});
		}
		let lhs = parse_collection_expr_full(lhs_src, self.scheme, self.allowed_kinds, self.raw)?;
		let rhs = parse_collection_expr_full(rhs_src, self.scheme, self.allowed_kinds, self.raw)?;
		self.pos = atom_end;
		Ok(Some(build_atom(
			LhsExpr::Collection(lhs),
			Op::Subset,
			Rhs::Collection(rhs),
			self.input[raw_start..self.pos].to_string(),
			self.raw,
		)?))
	}

	pub(super) fn parse_collection_call_body(
		&mut self,
		name: &str,
	) -> Result<CollectionExpr, ParseError> {
		if self.peek_byte() != Some(b'(') {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("expected `(` after `{name}`"),
			});
		}
		self.pos += 1;
		let expr = self.parse_collection_expr()?;
		self.skip_ws();
		if self.peek_byte() != Some(b')') {
			return Err(ParseError::BadExpr {
				expr: self.raw.to_string(),
				msg: format!("missing `)` for `{name}` at byte {}", self.pos),
			});
		}
		self.pos += 1;
		Ok(expr)
	}

	fn parse_collection_expr(&mut self) -> Result<CollectionExpr, ParseError> {
		let mut expr = self.parse_collection_primary()?;
		loop {
			self.skip_ws();
			let Some(op) = self.eat_collection_op() else {
				break;
			};
			let right = self.parse_collection_primary()?;
			expr = CollectionExpr::Binary {
				op,
				left: Box::new(expr),
				right: Box::new(right),
			};
		}
		Ok(expr)
	}

	fn parse_collection_primary(&mut self) -> Result<CollectionExpr, ParseError> {
		self.skip_ws();
		if self.input[self.pos..].starts_with("unique(") {
			self.pos += "unique".len();
			return self
				.parse_collection_call_body("unique")
				.map(|expr| CollectionExpr::Unique(Box::new(expr)));
		}
		if self.peek_byte() == Some(b'(') {
			self.pos += 1;
			let expr = self.parse_collection_expr()?;
			self.skip_ws();
			if self.peek_byte() != Some(b')') {
				return Err(ParseError::BadExpr {
					expr: self.raw.to_string(),
					msg: format!("missing `)` in collection expression at byte {}", self.pos),
				});
			}
			self.pos += 1;
			return Ok(expr);
		}
		let domain = self.parse_domain_ident()?;
		self.reject_pair_domain(&domain, "collection projections")?;
		let mut path = Vec::new();
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
					msg: format!(
						"expected collection projection segment at byte {}",
						self.pos
					),
				});
			}
			path.push(segment.to_string());
		}
		validate_collection_projection_path(&domain, &path, self.raw)?;
		Ok(CollectionExpr::Projection(CollectionProjection {
			domain,
			path,
		}))
	}

	fn eat_collection_op(&mut self) -> Option<CollectionOp> {
		if self.eat_keyword("intersect") {
			Some(CollectionOp::Intersect)
		} else if self.eat_keyword("union") {
			Some(CollectionOp::Union)
		} else if self.eat_keyword("diff") {
			Some(CollectionOp::Difference)
		} else {
			None
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
) -> Result<CollectionExpr, ParseError> {
	parse_collection_expr_full(s, scheme, allowed_kinds, full)
}

fn parse_collection_expr_full(
	s: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	full: &str,
) -> Result<CollectionExpr, ParseError> {
	let mut p = Parser::new(s, scheme, allowed_kinds, full);
	let expr = p.parse_collection_expr()?;
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
}
