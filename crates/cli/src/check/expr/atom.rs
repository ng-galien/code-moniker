use regex::Regex;

use code_moniker_core::core::uri::{UriConfig, from_uri};

use super::ast::*;
use super::collection::parse_collection_rhs;
use super::cursor::{lhs_token_end, operator_at};
use super::error::ParseError;
use super::number::parse_number_rhs;
use super::pairs::parse_pair_projection;
use crate::check::path;

pub(super) fn parse_atom(
	input: &str,
	scheme: &str,
	allowed_kinds: &[&str],
	full: &str,
	pair_bindings_allowed: bool,
) -> Result<Atom, ParseError> {
	let raw = input.trim().to_string();
	if let Some(atom) = parse_has_segment(&raw, full)? {
		return Ok(atom);
	}
	let (lhs_str, op_str, rhs_str) = split_atom(&raw, full)?;
	let lhs = parse_lhs(lhs_str, full, pair_bindings_allowed)?;
	let op = parse_op(op_str, full)?;
	let rhs = parse_rhs(
		rhs_str,
		op,
		scheme,
		allowed_kinds,
		full,
		pair_bindings_allowed,
	)?;
	build_atom(lhs, op, rhs, raw, full)
}

/// `has_segment("kind", "name")` is sugar for `moniker ~ '**/kind:name/**'`.
fn parse_has_segment(raw: &str, full: &str) -> Result<Option<Atom>, ParseError> {
	let Some(args) = raw
		.strip_prefix("has_segment(")
		.and_then(|s| s.strip_suffix(')'))
	else {
		return Ok(None);
	};
	let bail = |msg: String| ParseError::BadExpr {
		expr: full.to_string(),
		msg,
	};
	let mut parts = args.splitn(2, ',').map(str::trim);
	let kind = parts
		.next()
		.ok_or_else(|| bail("has_segment(kind, name) needs two args".to_string()))?;
	let name = parts
		.next()
		.ok_or_else(|| bail("has_segment(kind, name) needs two args".to_string()))?;
	let kind = unquote(kind);
	let name = unquote(name);
	if kind.is_empty() || name.is_empty() {
		return Err(bail(
			"has_segment(kind, name) args must be non-empty strings".to_string(),
		));
	}
	let pat_src = format!("**/{kind}:{name}/**");
	let pattern = path::parse(&pat_src).map_err(|e| bail(format!("{e}")))?;
	Ok(Some(build_atom(
		LhsExpr::Attr(Lhs::Moniker),
		Op::PathMatch,
		Rhs::PathPattern(pattern),
		raw.to_string(),
		full,
	)?))
}

pub(super) fn build_atom(
	lhs: LhsExpr,
	op: Op,
	rhs: Rhs,
	raw: String,
	full: &str,
) -> Result<Atom, ParseError> {
	check_type(&lhs, op, full)?;
	check_rhs_type(&lhs, &rhs, full)?;
	let regex = match (&op, &rhs) {
		(Op::RegexMatch | Op::RegexNoMatch, Rhs::RegexStr(p)) => {
			Some(Regex::new(p).map_err(|e| ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("invalid regex `{p}`: {e}"),
			})?)
		}
		_ => None,
	};
	Ok(Atom {
		lhs,
		op,
		rhs,
		raw,
		regex,
	})
}

pub(super) fn unquote(s: &str) -> &str {
	let s = s.trim();
	if (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
		|| (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
	{
		&s[1..s.len() - 1]
	} else {
		s
	}
}

/// Operator search restricted to the LHS prefix so an op char inside a regex
/// RHS (`^[a-z]+$`, `foo<=bar`) can't be mistaken for the main operator.
fn split_atom<'a>(s: &'a str, full: &str) -> Result<(&'a str, &'a str, &'a str), ParseError> {
	let bail = || ParseError::BadExpr {
		expr: full.to_string(),
		msg: format!("expected `<lhs> <op> <rhs>` in `{s}`"),
	};
	let lhs_end = lhs_token_end(s).ok_or_else(bail)?;
	let after_lhs = s[lhs_end..].trim_start();
	let op_offset = s.len() - after_lhs.len();
	if let Some((_op, op_len)) = operator_at(after_lhs) {
		let lhs = s[..lhs_end].trim();
		let rhs = after_lhs[op_len..].trim();
		if lhs.is_empty() || rhs.is_empty() {
			return Err(bail());
		}
		return Ok((lhs, &s[op_offset..op_offset + op_len], rhs));
	}
	Err(bail())
}

fn parse_lhs(s: &str, full: &str, pair_bindings_allowed: bool) -> Result<LhsExpr, ParseError> {
	if s.starts_with("count(") {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: "internal: count(...) reached parse_lhs; should be handled at primary level"
				.to_string(),
		});
	}
	if let Some(projection) = parse_pair_projection(s, full, pair_bindings_allowed)? {
		return Ok(LhsExpr::PairProjection(projection));
	}
	match Lhs::from_projection_name(s) {
		Some(lhs) if lhs.is_number_projection() => Ok(LhsExpr::Number(NumberExpr::Projection(lhs))),
		Some(lhs) => Ok(LhsExpr::Attr(lhs)),
		None => Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!("unknown lhs `{s}`"),
		}),
	}
}

pub(super) fn parse_op(s: &str, full: &str) -> Result<Op, ParseError> {
	Ok(match s {
		"=" => Op::Eq,
		"!=" => Op::Ne,
		"<" => Op::Lt,
		"<=" => Op::Le,
		">" => Op::Gt,
		">=" => Op::Ge,
		"=~" => Op::RegexMatch,
		"!~" => Op::RegexNoMatch,
		"@>" => Op::AncestorOf,
		"<@" => Op::DescendantOf,
		"?=" => Op::BindMatch,
		"~" => Op::PathMatch,
		"subset" => Op::Subset,
		other => {
			return Err(ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("unknown operator `{other}`"),
			});
		}
	})
}

fn check_type(lhs: &LhsExpr, op: Op, full: &str) -> Result<(), ParseError> {
	use Op::*;
	let lhs_attr = match lhs {
		LhsExpr::Attr(a) => *a,
		LhsExpr::Number(_) => {
			return match op {
				Lt | Le | Gt | Ge | Eq | Ne => Ok(()),
				_ => Err(ParseError::BadExpr {
					expr: full.to_string(),
					msg: format!("number expressions only accept numeric operators, got {op:?}"),
				}),
			};
		}
		LhsExpr::Collection(_) => {
			return match op {
				Subset => Ok(()),
				_ => Err(ParseError::BadExpr {
					expr: full.to_string(),
					msg: format!("collection expressions only accept subset operators, got {op:?}"),
				}),
			};
		}
		LhsExpr::PairProjection(projection) => projection.lhs,
		LhsExpr::Mode(_) => {
			return match op {
				Eq | Ne => Ok(()),
				_ => Err(ParseError::BadExpr {
					expr: full.to_string(),
					msg: format!("mode(...) only accepts equality operators, got {op:?}"),
				}),
			};
		}
		LhsExpr::SegmentOf { .. } => {
			return match op {
				Eq | Ne | RegexMatch | RegexNoMatch => Ok(()),
				_ => Err(ParseError::BadExpr {
					expr: full.to_string(),
					msg: format!("segment(...) only accepts string operators, got {op:?}"),
				}),
			};
		}
	};
	if !lhs_attr.accepts_op(op) {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: format!("operator {op:?} not valid for lhs {lhs_attr:?}"),
		});
	}
	Ok(())
}

fn check_rhs_type(lhs: &LhsExpr, rhs: &Rhs, full: &str) -> Result<(), ParseError> {
	if matches!(lhs, LhsExpr::Number(_)) && !matches!(rhs, Rhs::Number(_) | Rhs::PairProjection(_))
	{
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: "number expressions require numeric RHS".to_string(),
		});
	}
	if matches!(lhs, LhsExpr::Collection(_)) && !matches!(rhs, Rhs::Collection(_)) {
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: "collection expressions require collection RHS".to_string(),
		});
	}
	Ok(())
}

pub(super) fn parse_rhs(
	s: &str,
	op: Op,
	scheme: &str,
	allowed_kinds: &[&str],
	full: &str,
	pair_bindings_allowed: bool,
) -> Result<Rhs, ParseError> {
	let s = s.trim();
	let quoted = (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
		|| (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2);
	let s = if quoted { &s[1..s.len() - 1] } else { s };
	Ok(match op {
		Op::RegexMatch | Op::RegexNoMatch => Rhs::RegexStr(s.to_string()),
		Op::PathMatch => {
			let pattern = path::parse(s).map_err(|e| ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("{e}"),
			})?;
			Rhs::PathPattern(pattern)
		}
		Op::AncestorOf | Op::DescendantOf | Op::BindMatch => {
			let cfg = UriConfig { scheme };
			let m = from_uri(s, &cfg).map_err(|e| ParseError::BadExpr {
				expr: full.to_string(),
				msg: format!("invalid moniker URI `{s}`: {e}"),
			})?;
			Rhs::Moniker(m)
		}
		Op::Lt | Op::Le | Op::Gt | Op::Ge => {
			if let Some(projection) = parse_pair_projection(s, full, pair_bindings_allowed)? {
				Rhs::PairProjection(projection)
			} else {
				Rhs::Number(parse_number_rhs(
					s,
					scheme,
					allowed_kinds,
					full,
					pair_bindings_allowed,
				)?)
			}
		}
		Op::Subset => Rhs::Collection(parse_collection_rhs(
			s,
			scheme,
			allowed_kinds,
			full,
			pair_bindings_allowed,
		)?),
		Op::Eq | Op::Ne => {
			if quoted {
				Rhs::Str(s.to_string())
			} else if let Some(projection) = parse_pair_projection(s, full, pair_bindings_allowed)?
			{
				Rhs::PairProjection(projection)
			} else if let Ok(expr) =
				parse_number_rhs(s, scheme, allowed_kinds, full, pair_bindings_allowed)
			{
				Rhs::Number(expr)
			} else if s.contains("+moniker://") {
				let cfg = UriConfig { scheme };
				let m = from_uri(s, &cfg).map_err(|e| ParseError::BadExpr {
					expr: full.to_string(),
					msg: format!("invalid moniker URI `{s}`: {e}"),
				})?;
				Rhs::Moniker(m)
			} else if let Some(lhs) = Lhs::from_projection_name(s) {
				Rhs::Projection(lhs)
			} else {
				Rhs::Str(s.to_string())
			}
		}
	})
}

#[cfg(test)]
mod tests {
	use super::super::parse;
	use super::super::test_support::{KINDS, TS, solo};
	use super::super::*;

	#[test]
	fn parses_name_regex() {
		let e = parse("name =~ ^[A-Z]", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.lhs, LhsExpr::Attr(Lhs::Name)));
		assert!(matches!(a.op, Op::RegexMatch));
		assert!(matches!(a.rhs, Rhs::RegexStr(_)));
		assert!(a.regex.is_some());
	}

	#[test]
	fn parses_shape_eq() {
		let e = parse("shape = 'callable'", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.lhs, LhsExpr::Attr(Lhs::Shape)));
		assert!(matches!(a.op, Op::Eq));
		match &a.rhs {
			Rhs::Str(s) => assert_eq!(s, "callable"),
			other => panic!("expected Str rhs, got {other:?}"),
		}
	}

	#[test]
	fn parses_parent_shape_eq() {
		let e = parse("parent.shape = 'type'", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.lhs, LhsExpr::Attr(Lhs::ParentShape)));
	}

	#[test]
	fn parses_target_shape_regex() {
		let e = parse("target.shape =~ ^(type|callable)$", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.lhs, LhsExpr::Attr(Lhs::TargetShape)));
		assert!(matches!(a.op, Op::RegexMatch));
	}

	#[test]
	fn shape_rejects_numeric_operator() {
		assert!(parse("shape < 'callable'", TS, KINDS).is_err());
	}

	#[test]
	fn parses_moniker_descendant() {
		let e = parse("moniker <@ code+moniker://./class:Foo", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op, &a.rhs) {
			(LhsExpr::Attr(Lhs::Moniker), Op::DescendantOf, Rhs::Moniker(_)) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_source_target_parent_moniker_projection() {
		let e = parse("source.parent = target.parent", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.rhs) {
			(
				LhsExpr::Attr(Lhs::SourceParentMoniker),
				Rhs::Projection(Lhs::TargetParentMoniker),
			) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn rejects_op_lhs_type_mismatch() {
		let r = parse("lines =~ foo", TS, KINDS);
		assert!(r.is_err(), "lines is numeric, =~ should be rejected");
	}

	#[test]
	fn rejects_unknown_lhs() {
		let r = parse("bogus = foo", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn rejects_invalid_regex() {
		let r = parse("name =~ [unclosed", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn rejects_invalid_moniker_uri() {
		let r = parse("moniker <@ not-a-uri", TS, KINDS);
		assert!(r.is_err());
	}

	#[test]
	fn regex_rhs_containing_op_chars_is_not_split_on_rhs() {
		let e = parse("text =~ ^count\\(.+\\) <= 20$", TS, KINDS).unwrap();
		let a = solo(&e);
		match (&a.lhs, &a.op) {
			(LhsExpr::Attr(Lhs::Text), Op::RegexMatch) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn regex_rhs_with_neq_token_is_not_split_on_rhs() {
		let e = parse("text =~ foo!=bar", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.op, Op::RegexMatch));
		match &a.rhs {
			Rhs::RegexStr(s) => assert_eq!(s, "foo!=bar"),
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn strips_surrounding_quotes_on_rhs() {
		let e = parse("name =~ \"^foo$\"", TS, KINDS).unwrap();
		match &solo(&e).rhs {
			Rhs::RegexStr(s) => assert_eq!(s, "^foo$"),
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn quoted_count_like_rhs_stays_string_literal() {
		let e = parse("name = 'count(method)'", TS, KINDS).unwrap();
		match &solo(&e).rhs {
			Rhs::Str(s) => assert_eq!(s, "count(method)"),
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn parses_path_match() {
		let e = parse("moniker ~ '**/class:Foo/**'", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.op, Op::PathMatch));
		assert!(matches!(a.rhs, Rhs::PathPattern(_)));
	}

	#[test]
	fn parses_path_match_with_regex_step() {
		let e = parse("moniker ~ '**/class:/Port$/'", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.op, Op::PathMatch));
	}

	#[test]
	fn has_segment_desugars_to_path_match() {
		let e = parse("has_segment('module', 'domain')", TS, KINDS).unwrap();
		let a = solo(&e);
		assert!(matches!(a.op, Op::PathMatch));
		match &a.rhs {
			Rhs::PathPattern(p) => assert_eq!(p.raw, "**/module:domain/**"),
			other => panic!("expected PathPattern, got {other:?}"),
		}
	}

	#[test]
	fn rejects_path_match_on_non_moniker_lhs() {
		assert!(parse("name ~ 'foo'", TS, KINDS).is_err());
	}

	#[test]
	fn rejects_invalid_path_pattern() {
		assert!(parse("moniker ~ ''", TS, KINDS).is_err());
		assert!(parse("moniker ~ 'no-colon-step'", TS, KINDS).is_err());
	}
}
