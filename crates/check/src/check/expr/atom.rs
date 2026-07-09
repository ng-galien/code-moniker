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
	if matches!(lhs, LhsExpr::Number(_))
		&& matches!(rhs, Rhs::CurrentProjection(projection) if !projection.is_number_projection())
	{
		return Err(ParseError::BadExpr {
			expr: full.to_string(),
			msg: "number expressions require numeric current projections".to_string(),
		});
	}
	if matches!(lhs, LhsExpr::Number(_))
		&& !matches!(
			rhs,
			Rhs::Number(_) | Rhs::PairProjection(_) | Rhs::CurrentProjection(_)
		) {
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
			} else if let Some(lhs) = parse_current_projection(s) {
				Rhs::CurrentProjection(lhs)
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
			} else if let Some(lhs) = parse_current_projection(s) {
				Rhs::CurrentProjection(lhs)
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

fn parse_current_projection(s: &str) -> Option<Lhs> {
	s.strip_prefix("current.")
		.and_then(Lhs::from_projection_name)
}
