use super::expect::ExpectedViolation;
use super::{Scenario, ScenarioFile, ScenarioMeta};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioError {
	pub line: usize,
	pub message: String,
}

impl std::fmt::Display for ScenarioError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "scenario line {}: {}", self.line, self.message)
	}
}

impl std::error::Error for ScenarioError {}

struct Line<'a> {
	no: usize,
	start: usize,
	text: &'a str,
}

enum Block<'a> {
	Rules,
	Expect,
	File(&'a str),
	Ignored,
}

pub(super) fn parse_document(document: &str) -> Result<Scenario, ScenarioError> {
	let lines = split_lines(document);
	let mut scenario = Scenario {
		meta: ScenarioMeta::default(),
		rules: None,
		files: Vec::new(),
		expects: Vec::new(),
		expect_span: None,
	};
	let mut cursor = parse_front_matter(&lines, &mut scenario.meta)?;
	while cursor < lines.len() {
		let line = &lines[cursor];
		let Some(fence) = fence_length(line.text) else {
			cursor += 1;
			continue;
		};
		let close = closing_fence(&lines, cursor + 1, fence).ok_or_else(|| ScenarioError {
			line: line.no,
			message: "unterminated code fence".to_string(),
		})?;
		let span = content_span(&lines, cursor, close);
		collect_block(document, &mut scenario, line, span)?;
		cursor = close + 1;
	}
	Ok(scenario)
}

fn collect_block(
	document: &str,
	scenario: &mut Scenario,
	opening: &Line<'_>,
	span: (usize, usize),
) -> Result<(), ScenarioError> {
	let content = &document[span.0..span.1];
	match classify_info_string(opening.text) {
		Block::Rules => {
			if scenario.rules.is_some() {
				return Err(block_error(opening, "duplicate cm:rules block"));
			}
			scenario.rules = Some(content.to_string());
		}
		Block::Expect => {
			if scenario.expect_span.is_some() {
				return Err(block_error(opening, "duplicate cm:expect block"));
			}
			scenario.expect_span = Some(span);
			scenario.expects = parse_expect_block(content, opening.no)?;
		}
		Block::File(path) => {
			validate_relative_path(path, opening.no)?;
			if scenario.files.iter().any(|file| file.path == path) {
				return Err(block_error(opening, &format!("duplicate file `{path}`")));
			}
			scenario.files.push(ScenarioFile {
				path: path.to_string(),
				body: content.to_string(),
			});
		}
		Block::Ignored => {}
	}
	Ok(())
}

fn split_lines(document: &str) -> Vec<Line<'_>> {
	let mut lines = Vec::new();
	let mut start = 0;
	for (no, text) in document.split_inclusive('\n').enumerate() {
		lines.push(Line {
			no: no + 1,
			start,
			text: text.trim_end_matches(['\n', '\r']),
		});
		start += text.len();
	}
	lines
}

fn parse_front_matter(lines: &[Line<'_>], meta: &mut ScenarioMeta) -> Result<usize, ScenarioError> {
	if lines.first().is_none_or(|line| line.text.trim() != "---") {
		return Ok(0);
	}
	let close = lines
		.iter()
		.skip(1)
		.position(|line| line.text.trim() == "---")
		.ok_or_else(|| ScenarioError {
			line: 1,
			message: "unterminated front matter".to_string(),
		})?;
	for line in &lines[1..close + 1] {
		parse_meta_line(line, meta)?;
	}
	Ok(close + 2)
}

fn parse_meta_line(line: &Line<'_>, meta: &mut ScenarioMeta) -> Result<(), ScenarioError> {
	let text = line.text.trim();
	if text.is_empty() || text.starts_with('#') {
		return Ok(());
	}
	let (key, value) = text.split_once(':').ok_or_else(|| ScenarioError {
		line: line.no,
		message: format!("expected `key: value` in front matter, got `{text}`"),
	})?;
	let value = value.trim();
	match key.trim() {
		"name" => meta.name = value.to_string(),
		"lang" => meta.lang = value.to_string(),
		"blurb" => meta.blurb = value.to_string(),
		"published" => meta.published = parse_bool(value, line)?,
		"default_rules" => meta.default_rules = Some(parse_bool(value, line)?),
		key => {
			return Err(ScenarioError {
				line: line.no,
				message: format!("unknown front matter key `{key}`"),
			});
		}
	}
	Ok(())
}

fn parse_bool(value: &str, line: &Line<'_>) -> Result<bool, ScenarioError> {
	match value {
		"true" => Ok(true),
		"false" => Ok(false),
		value => Err(ScenarioError {
			line: line.no,
			message: format!("expected `true` or `false`, got `{value}`"),
		}),
	}
}

fn fence_length(text: &str) -> Option<usize> {
	let length = text.bytes().take_while(|byte| *byte == b'`').count();
	(length >= 3).then_some(length)
}

fn closing_fence(lines: &[Line<'_>], from: usize, fence: usize) -> Option<usize> {
	lines[from..]
		.iter()
		.position(|line| {
			fence_length(line.text).is_some_and(|length| length >= fence)
				&& line.text.trim_end().trim_matches('`').is_empty()
		})
		.map(|offset| from + offset)
}

fn content_span(lines: &[Line<'_>], opening: usize, closing: usize) -> (usize, usize) {
	if opening + 1 >= closing {
		return (lines[closing].start, lines[closing].start);
	}
	(lines[opening + 1].start, lines[closing].start)
}

fn classify_info_string(text: &str) -> Block<'_> {
	let info = text.trim_start_matches('`').trim();
	for token in info.split_whitespace() {
		if token == "cm:rules" {
			return Block::Rules;
		}
		if token == "cm:expect" {
			return Block::Expect;
		}
		if let Some(path) = token.strip_prefix("cm:file=") {
			return Block::File(path);
		}
	}
	Block::Ignored
}

fn parse_expect_block(
	content: &str,
	opening_line: usize,
) -> Result<Vec<ExpectedViolation>, ScenarioError> {
	let mut expects = Vec::new();
	for (offset, line) in content.lines().enumerate() {
		let text = line.trim();
		if text.is_empty() || text.starts_with('#') {
			continue;
		}
		let expected = ExpectedViolation::parse(text).map_err(|message| ScenarioError {
			line: opening_line + offset + 1,
			message,
		})?;
		expects.push(expected);
	}
	expects.sort();
	Ok(expects)
}

fn validate_relative_path(path: &str, line: usize) -> Result<(), ScenarioError> {
	let invalid = path.is_empty()
		|| path.starts_with('/')
		|| path.contains('\\')
		|| path.contains(':')
		|| path
			.split('/')
			.any(|component| matches!(component, "" | "." | ".."));
	if invalid {
		return Err(ScenarioError {
			line,
			message: format!("`{path}` must be a clean relative path (no `..`, `.`, or absolute)"),
		});
	}
	Ok(())
}

fn block_error(opening: &Line<'_>, message: &str) -> ScenarioError {
	ScenarioError {
		line: opening.no,
		message: message.to_string(),
	}
}
