//! Path patterns matched against the segments of a moniker.
//!
//! `'module:domain'`, `'**/class:*'`, `'class:/^[A-Z].*Port$/'`, `'*:/^(api|web)$/'`, `'**/module:a/**'`.
//! `**` matches zero or more segments ; the other step forms match exactly one.

use regex::Regex;

use code_moniker_core::core::moniker::Moniker;

#[derive(Debug, Clone)]
pub(crate) enum Step {
	Literal {
		kind: Vec<u8>,
		name: Vec<u8>,
	},
	Segment {
		kind: SegmentMatch,
		name: SegmentMatch,
	},
	AnySegment,
	DoubleStar,
}

#[derive(Debug, Clone)]
pub(crate) enum SegmentMatch {
	Any,
	Literal(Vec<u8>),
	Regex(Regex),
}

#[derive(Debug, Clone)]
pub(crate) struct Pattern {
	pub steps: Vec<Step>,
	pub raw: String,
}

#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum PatternError {
	#[error("path pattern `{pattern}`: {msg}")]
	Bad { pattern: String, msg: String },
}

pub(crate) fn parse(input: &str) -> Result<Pattern, PatternError> {
	let raw = input.to_string();
	if input.is_empty() {
		return Err(PatternError::Bad {
			pattern: raw,
			msg: "empty path pattern".to_string(),
		});
	}
	let steps = split_outer(input)
		.map_err(|msg| PatternError::Bad {
			pattern: raw.clone(),
			msg: msg.to_string(),
		})?
		.into_iter()
		.map(|step| parse_step(step, &raw))
		.collect::<Result<_, _>>()?;
	Ok(Pattern { steps, raw })
}

fn split_outer(mut input: &str) -> Result<Vec<&str>, &'static str> {
	let mut steps = Vec::new();
	loop {
		let (end, next) = step_end(input)?;
		steps.push(&input[..end]);
		let Some(next) = next else {
			return Ok(steps);
		};
		input = &input[next..];
	}
}

fn step_end(input: &str) -> Result<(usize, Option<usize>), &'static str> {
	if let Some(colon) = input.find([':', '/'])
		&& input.as_bytes()[colon] == b':'
	{
		let start = colon + 1;
		match input.as_bytes().get(start) {
			Some(b'/') => {
				let end = delimited_name_end(input, start, b'/').ok_or("unterminated regex")?;
				// A regex's closing slash also separates it from the next step.
				return Ok((end, (end < input.len()).then_some(end)));
			}
			Some(b'`') => {
				let end = delimited_name_end(input, start, b'`')
					.ok_or("unterminated backtick-quoted name")?;
				return match input.as_bytes().get(end) {
					None => Ok((end, None)),
					Some(b'/') => Ok((end, Some(end + 1))),
					_ => Err("unexpected data after backtick-quoted name"),
				};
			}
			_ => {}
		}
	}
	Ok(input
		.find('/')
		.map_or((input.len(), None), |end| (end, Some(end + 1))))
}

fn delimited_name_end(input: &str, start: usize, delimiter: u8) -> Option<usize> {
	let mut bytes = input.as_bytes()[start + 1..].iter().enumerate().peekable();
	while let Some((offset, &byte)) = bytes.next() {
		if delimiter == b'/' && byte == b'\\' {
			bytes.next();
		} else if byte == delimiter {
			if delimiter == b'`' && bytes.peek().is_some_and(|(_, next)| **next == b'`') {
				bytes.next();
			} else {
				return Some(start + offset + 2);
			}
		}
	}
	None
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
	let kind = segment_match(kind, full)?;
	let name = if let Some(literal) = name.strip_prefix('`').and_then(|n| n.strip_suffix('`')) {
		SegmentMatch::Literal(literal.replace("``", "`").into_bytes())
	} else if let Some(regex) = name.strip_prefix('/').and_then(|n| n.strip_suffix('/')) {
		SegmentMatch::Regex(segment_regex(regex, full)?)
	} else {
		segment_match(name, full)?
	};
	Ok(match (kind, name) {
		(SegmentMatch::Literal(kind), SegmentMatch::Literal(name)) => Step::Literal { kind, name },
		(kind, name) => Step::Segment { kind, name },
	})
}

fn segment_match(value: &str, full: &str) -> Result<SegmentMatch, PatternError> {
	if value == "*" {
		return Ok(SegmentMatch::Any);
	}
	if !value.contains('*') {
		return Ok(SegmentMatch::Literal(value.as_bytes().to_vec()));
	}
	let regex = value
		.split('*')
		.map(regex::escape)
		.collect::<Vec<_>>()
		.join(".*");
	segment_regex(&format!(r"(?s:\A{regex}\z)"), full).map(SegmentMatch::Regex)
}

fn segment_regex(regex: &str, full: &str) -> Result<Regex, PatternError> {
	Regex::new(regex).map_err(|error| PatternError::Bad {
		pattern: full.to_string(),
		msg: format!("invalid regex `{regex}`: {error}"),
	})
}

/// Matches `pattern` against the segments of `m`. `**` is greedy non-deterministic
/// — recursive backtracking, O(2^n) worst case but moniker depth ≤ ~10 in practice.
pub(crate) fn matches(pattern: &Pattern, m: &Moniker) -> bool {
	let view = m.as_view();
	let segs: Vec<(&[u8], &[u8])> = view.segments().map(|s| (s.kind, s.name)).collect();
	match_steps(&pattern.steps, &segs)
}

pub(crate) fn matches_text_segments(pattern: &Pattern, segments: &[(&str, &str)]) -> bool {
	let segments = segments
		.iter()
		.map(|(kind, name)| (kind.as_bytes(), name.as_bytes()))
		.collect::<Vec<_>>();
	match_steps(&pattern.steps, &segments)
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
		Step::Segment { kind, name } => matches_segment(kind, k) && matches_segment(name, n),
		Step::AnySegment => true,
		Step::DoubleStar => unreachable!("DoubleStar handled in match_steps"),
	}
}

fn matches_segment(pattern: &SegmentMatch, value: &[u8]) -> bool {
	match pattern {
		SegmentMatch::Any => true,
		SegmentMatch::Literal(literal) => value == literal,
		SegmentMatch::Regex(regex) => {
			std::str::from_utf8(value).is_ok_and(|value| regex.is_match(value))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use code_moniker_core::core::moniker::MonikerBuilder;

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
		if !matches(&p, m) {
			panic!("pattern `{pat}` should match {m:?}");
		}
	}

	fn assert_no_match(pat: &str, m: &Moniker) {
		let p = parse(pat).expect("pattern parses");
		if matches(&p, m) {
			panic!("pattern `{pat}` should NOT match {m:?}");
		}
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
	fn regex_step_can_match_any_kind_name() {
		let module = build(&[(b"module", b"java")]);
		let path = build(&[(b"path", b"rust")]);
		let other = build(&[(b"path", b"kotlin")]);

		assert_match("*:/^(java|rust)$/", &module);
		assert_match("*:/^(java|rust)$/", &path);
		assert_no_match("*:/^(java|rust)$/", &other);
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

	#[test]
	fn quoted_names_match_literal_slash_backtick_and_star() {
		let m = build(&[(b"external_pkg", b"@example/a`b*"), (b"path", b"adapt")]);
		assert_match("**/external_pkg:`@example/a``b*`/**", &m);
		assert_no_match("**/external_pkg:`@example/a``b`/**", &m);
		assert_no_match("**/external_pkg:`*`/**", &m);
		let star = build(&[(b"path", b"*")]);
		assert_match("path:`*`", &star);
	}

	#[test]
	fn partial_stars_match_one_segment_with_literal_punctuation() {
		let m = build(&[
			(b"external_pkg", b"@example/driver.js"),
			(b"path", b"adapt"),
		]);
		assert_match("external_*:*example*.js/**", &m);
		assert_match(r"external_*:/^@example\/driver\.js$/**", &m);
		assert_no_match("external_*:*example*.ts/**", &m);
		assert_no_match("extern_*:example*/**", &m);
		assert_no_match("module*:*example*/**", &m);

		let methods = build(&[(b"method", b"success(1)")]);
		assert_match("method:success**", &methods);
		assert_no_match("method:`success**`", &methods);
	}

	#[test]
	fn rejects_unclosed_or_continued_quoted_names() {
		assert!(parse("dir:`unterminated/**").is_err());
		assert!(parse("dir:`name`tail/**").is_err());
		assert!(parse("dir:/unterminated").is_err());
	}
}
