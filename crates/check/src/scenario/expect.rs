use std::fmt;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ExpectedViolation {
	pub path: String,
	pub lines: (u32, u32),
	pub rule_id: String,
}

impl ExpectedViolation {
	pub fn parse(line: &str) -> Result<Self, String> {
		parse_expected(line)
	}
}

impl fmt::Display for ExpectedViolation {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{} @ {}:{}",
			self.rule_id,
			self.path,
			format_span(self.lines)
		)
	}
}

fn format_span((start, end): (u32, u32)) -> String {
	if start == end {
		format!("L{start}")
	} else {
		format!("L{start}-L{end}")
	}
}

fn parse_expected(line: &str) -> Result<ExpectedViolation, String> {
	let (rule_id, location) = line
		.split_once('@')
		.ok_or_else(|| expectation_syntax(line))?;
	let rule_id = rule_id.trim();
	let (path, span) = location
		.trim()
		.rsplit_once(':')
		.ok_or_else(|| expectation_syntax(line))?;
	if rule_id.is_empty() || path.is_empty() {
		return Err(expectation_syntax(line));
	}
	Ok(ExpectedViolation {
		rule_id: rule_id.to_string(),
		path: path.to_string(),
		lines: parse_span(span).ok_or_else(|| expectation_syntax(line))?,
	})
}

fn parse_span(span: &str) -> Option<(u32, u32)> {
	let span = span.strip_prefix('L')?;
	match span.split_once("-L") {
		Some((start, end)) => Some((start.parse().ok()?, end.parse().ok()?)),
		None => {
			let line = span.parse().ok()?;
			Some((line, line))
		}
	}
}

fn expectation_syntax(line: &str) -> String {
	format!("expected `<rule-id> @ <path>:L<start>[-L<end>]`, got `{line}`")
}
