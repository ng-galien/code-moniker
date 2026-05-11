//! Path patterns matched against the segments of a moniker.
//!
//! `'module:domain'`, `'**/class:*'`, `'class:/^[A-Z].*Port$/'`, `'**/module:a/**'`.
//! `**` matches zero or more segments ; the other step forms match exactly one.

use regex::Regex;

use crate::core::moniker::Moniker;

#[derive(Debug, Clone)]
pub enum Step {
	Literal { kind: Vec<u8>, name: Vec<u8> },
	KindWildcard(Vec<u8>),
	NameWildcard(Vec<u8>),
	AnySegment,
	Regex { kind: Vec<u8>, re: Regex },
	DoubleStar,
}

#[derive(Debug, Clone)]
pub struct Pattern {
	pub steps: Vec<Step>,
	pub raw: String,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PatternError {
	#[error("path pattern `{pattern}`: {msg}")]
	Bad { pattern: String, msg: String },
}

pub fn parse(input: &str) -> Result<Pattern, PatternError> {
	let raw = input.to_string();
	if input.is_empty() {
		return Err(PatternError::Bad {
			pattern: raw,
			msg: "empty path pattern".to_string(),
		});
	}
	let mut steps = Vec::new();
	for raw_step in split_outer(input) {
		steps.push(parse_step(&raw_step, &raw)?);
	}
	Ok(Pattern { steps, raw })
}

/// Split on `/` but skip slashes inside a `/.../` regex run. A regex starts
/// right after `:`, ends at the next unescaped `/`. The closing `/` is part
/// of the step (so `class:/Entity$/` is one step), and the next character
/// starts a new step without needing another separator.
fn split_outer(s: &str) -> Vec<String> {
	let mut out = Vec::new();
	let mut buf = String::new();
	let mut in_regex = false;
	let mut prev_was_colon = false;
	let mut prev_was_backslash = false;
	let mut just_closed_regex = false;
	for c in s.chars() {
		if just_closed_regex {
			out.push(std::mem::take(&mut buf));
			just_closed_regex = false;
		}
		if !in_regex && c == '/' && !prev_was_colon {
			out.push(std::mem::take(&mut buf));
			prev_was_colon = false;
			prev_was_backslash = false;
			continue;
		}
		buf.push(c);
		if in_regex {
			if c == '/' && !prev_was_backslash {
				in_regex = false;
				just_closed_regex = true;
			}
		} else if c == '/' && prev_was_colon {
			in_regex = true;
		}
		prev_was_colon = c == ':';
		prev_was_backslash = c == '\\' && !prev_was_backslash;
	}
	out.push(buf);
	out
}

fn parse_step(s: &str, full: &str) -> Result<Step, PatternError> {
	if s == "**" {
		return Ok(Step::DoubleStar);
	}
	if s == "*" {
		return Ok(Step::AnySegment);
	}
	let Some(colon) = s.find(':') else {
		return Err(PatternError::Bad {
			pattern: full.to_string(),
			msg: format!("step `{s}` is missing the `kind:name` separator"),
		});
	};
	let kind = &s[..colon];
	let name = &s[colon + 1..];
	if kind.is_empty() {
		return Err(PatternError::Bad {
			pattern: full.to_string(),
			msg: format!("step `{s}` has empty kind"),
		});
	}
	if name.is_empty() {
		return Err(PatternError::Bad {
			pattern: full.to_string(),
			msg: format!("step `{s}` has empty name"),
		});
	}
	if kind == "*" {
		return Ok(Step::NameWildcard(name.as_bytes().to_vec()));
	}
	if name == "*" {
		return Ok(Step::KindWildcard(kind.as_bytes().to_vec()));
	}
	if let Some(stripped) = name.strip_prefix('/').and_then(|r| r.strip_suffix('/')) {
		let re = Regex::new(stripped).map_err(|e| PatternError::Bad {
			pattern: full.to_string(),
			msg: format!("invalid regex `{stripped}`: {e}"),
		})?;
		return Ok(Step::Regex {
			kind: kind.as_bytes().to_vec(),
			re,
		});
	}
	Ok(Step::Literal {
		kind: kind.as_bytes().to_vec(),
		name: name.as_bytes().to_vec(),
	})
}

/// Matches `pattern` against the segments of `m`. `**` is greedy non-deterministic
/// — recursive backtracking, O(2^n) worst case but moniker depth ≤ ~10 in practice.
pub fn matches(pattern: &Pattern, m: &Moniker) -> bool {
	let view = m.as_view();
	let segs: Vec<(&[u8], &[u8])> = view.segments().map(|s| (s.kind, s.name)).collect();
	match_steps(&pattern.steps, &segs)
}

fn match_steps(steps: &[Step], segs: &[(&[u8], &[u8])]) -> bool {
	match steps.split_first() {
		None => segs.is_empty(),
		Some((Step::DoubleStar, rest)) => (0..=segs.len()).any(|k| match_steps(rest, &segs[k..])),
		Some((step, rest)) => match segs.split_first() {
			None => false,
			Some((seg, segs_rest)) => match_step(step, seg) && match_steps(rest, segs_rest),
		},
	}
}

fn match_step(step: &Step, seg: &(&[u8], &[u8])) -> bool {
	let (k, n) = *seg;
	match step {
		Step::Literal { kind, name } => k == kind.as_slice() && n == name.as_slice(),
		Step::KindWildcard(kind) => k == kind.as_slice(),
		Step::NameWildcard(name) => n == name.as_slice(),
		Step::AnySegment => true,
		Step::Regex { kind, re } => {
			k == kind.as_slice() && {
				match std::str::from_utf8(n) {
					Ok(s) => re.is_match(s),
					Err(_) => false,
				}
			}
		}
		Step::DoubleStar => unreachable!("DoubleStar handled in match_steps"),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::moniker::MonikerBuilder;

	fn build(steps: &[(&[u8], &[u8])]) -> Moniker {
		let mut b = MonikerBuilder::new();
		b.project(b".");
		for (k, n) in steps {
			b.segment(k, n);
		}
		b.build()
	}

	fn assert_match(pat: &str, m: &Moniker) {
		let p = parse(pat).expect("pattern parses");
		assert!(matches(&p, m), "pattern `{pat}` should match {m:?}");
	}

	fn assert_no_match(pat: &str, m: &Moniker) {
		let p = parse(pat).expect("pattern parses");
		assert!(!matches(&p, m), "pattern `{pat}` should NOT match {m:?}");
	}

	#[test]
	fn literal_anchored_matches_exact() {
		let m = build(&[(b"lang", b"ts"), (b"module", b"domain")]);
		assert_match("lang:ts/module:domain", &m);
	}

	#[test]
	fn literal_anchored_does_not_match_with_extra_tail() {
		let m = build(&[(b"lang", b"ts"), (b"module", b"domain"), (b"class", b"Foo")]);
		// Without `/**`, pattern must match the WHOLE path.
		assert_no_match("lang:ts/module:domain", &m);
	}

	#[test]
	fn double_star_matches_any_depth() {
		let m = build(&[(b"lang", b"ts"), (b"module", b"a"), (b"class", b"Foo")]);
		assert_match("**/class:Foo", &m);
		assert_match("**/class:Foo/**", &m);
		assert_match("lang:ts/**/class:Foo", &m);
	}

	#[test]
	fn double_star_matches_zero_segments() {
		let m = build(&[(b"class", b"Foo")]);
		assert_match("**/class:Foo", &m); // `**` matches 0 segments
	}

	#[test]
	fn kind_wildcard_matches_any_name() {
		let m = build(&[(b"lang", b"ts"), (b"class", b"Anything")]);
		assert_match("lang:ts/class:*", &m);
	}

	#[test]
	fn name_wildcard_matches_any_kind() {
		let m1 = build(&[(b"lang", b"ts"), (b"class", b"Foo")]);
		let m2 = build(&[(b"lang", b"ts"), (b"interface", b"Foo")]);
		assert_match("lang:ts/*:Foo", &m1);
		assert_match("lang:ts/*:Foo", &m2);
	}

	#[test]
	fn any_segment_matches_one() {
		let m = build(&[(b"lang", b"ts"), (b"module", b"x"), (b"class", b"Y")]);
		assert_match("lang:ts/*/class:Y", &m);
	}

	#[test]
	fn regex_step_matches_name() {
		let m = build(&[(b"class", b"UserPort")]);
		assert_match("class:/Port$/", &m);
		assert_no_match("class:/Adapter$/", &m);
	}

	#[test]
	fn ddd_aliases_against_real_moniker_shape() {
		let m = build(&[
			(b"lang", b"ts"),
			(b"module", b"domain"),
			(b"class", b"OrderEntity"),
			(b"method", b"validate"),
		]);
		assert_match("**/module:domain/**", &m);
		// `**/class:/Entity$/**` — has a class:…Entity segment somewhere.
		// Without the trailing `**`, the pattern requires the regex step to
		// be the LAST segment.
		assert_match("**/class:/Entity$/**", &m);
		assert_match("**/class:/Entity$/method:*", &m);
		assert_no_match("**/module:infrastructure/**", &m);
	}

	#[test]
	fn rejects_empty_pattern() {
		assert!(parse("").is_err());
	}

	#[test]
	fn rejects_step_without_colon() {
		assert!(parse("foo/bar").is_err());
	}

	#[test]
	fn rejects_bad_regex() {
		assert!(parse("class:/[unclosed/").is_err());
	}
}
