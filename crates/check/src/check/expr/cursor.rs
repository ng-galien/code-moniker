use super::error::ParseError;

#[derive(Clone)]
pub(super) struct ParserState<'a> {
	input: &'a str,
	pos: usize,
	scheme: &'a str,
	allowed_kinds: &'a [&'a str],
	raw: &'a str,
	pair_bindings_allowed: bool,
}

pub(super) type ParseResult<'a, T> = Result<(T, ParserState<'a>), ParseError>;

const TWO_CHAR_OPS: &[&str] = &["<=", ">=", "!=", "=~", "!~", "<@", "@>", "?="];
const ONE_CHAR_OPS: &[&str] = &["<", ">", "=", "~"];

pub(super) fn lhs_token_end(input: &str) -> Option<usize> {
	let bytes = input.as_bytes();
	let mut i = 0;
	while i < bytes.len() && bytes[i].is_ascii_whitespace() {
		i += 1;
	}
	let start = i;
	while i < bytes.len()
		&& (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'.')
	{
		i += 1;
	}
	if i == start {
		return None;
	}
	if i < bytes.len() && bytes[i] == b'(' {
		i += 1;
		while i < bytes.len() && bytes[i] != b')' {
			i += 1;
		}
		if i == bytes.len() {
			return None;
		}
		i += 1;
	}
	Some(i)
}

pub(super) fn operator_at(input: &str) -> Option<(&'static str, usize)> {
	for op in TWO_CHAR_OPS {
		if input.starts_with(op) {
			return Some((*op, op.len()));
		}
	}
	for op in ONE_CHAR_OPS {
		if input.starts_with(op) {
			return Some((*op, op.len()));
		}
	}
	None
}

impl<'a> ParserState<'a> {
	pub(super) fn new(
		input: &'a str,
		scheme: &'a str,
		allowed_kinds: &'a [&'a str],
		raw: &'a str,
	) -> Self {
		Self {
			input,
			pos: 0,
			scheme,
			allowed_kinds,
			raw,
			pair_bindings_allowed: false,
		}
	}
}

pub(super) fn raw<'a>(state: &ParserState<'a>) -> &'a str {
	state.raw
}

pub(super) fn scheme<'a>(state: &ParserState<'a>) -> &'a str {
	state.scheme
}

pub(super) fn allowed_kinds<'a>(state: &ParserState<'a>) -> &'a [&'a str] {
	state.allowed_kinds
}

pub(super) fn pair_bindings_allowed(state: &ParserState<'_>) -> bool {
	state.pair_bindings_allowed
}

pub(super) fn with_pair_bindings_allowed<'a>(
	mut state: ParserState<'a>,
	allowed: bool,
) -> ParserState<'a> {
	state.pair_bindings_allowed = allowed;
	state
}

pub(super) fn position(state: &ParserState<'_>) -> usize {
	state.pos
}

pub(super) fn rest<'a>(state: &ParserState<'a>) -> &'a str {
	&state.input[state.pos..]
}

pub(super) fn is_at_end(state: &ParserState<'_>) -> bool {
	state.pos >= state.input.len()
}

pub(super) fn bail<S: Into<String>>(state: &ParserState<'_>, msg: S) -> ParseError {
	ParseError::BadExpr {
		expr: state.raw.to_string(),
		msg: msg.into(),
	}
}

pub(super) fn skip_ws<'a>(mut state: ParserState<'a>) -> ParserState<'a> {
	let bytes = state.input.as_bytes();
	while state.pos < bytes.len() && bytes[state.pos].is_ascii_whitespace() {
		state.pos += 1;
	}
	state
}

pub(super) fn peek_byte(state: &ParserState<'_>) -> Option<u8> {
	state.input.as_bytes().get(state.pos).copied()
}

pub(super) fn starts_with(state: &ParserState<'_>, prefix: &str) -> bool {
	rest(state).starts_with(prefix)
}

pub(super) fn advance<'a>(mut state: ParserState<'a>, len: usize) -> ParserState<'a> {
	state.pos += len;
	state
}

pub(super) fn eat_keyword<'a>(mut state: ParserState<'a>, kw: &str) -> (bool, ParserState<'a>) {
	let remaining = rest(&state);
	if let Some(after) = remaining.strip_prefix(kw) {
		let next_ok = after.is_empty()
			|| after.starts_with(|c: char| c.is_ascii_whitespace())
			|| after.starts_with('(');
		if next_ok {
			state.pos += kw.len();
			return (true, state);
		}
	}
	(false, state)
}

pub(super) fn operator(state: &ParserState<'_>) -> Option<(&'static str, usize)> {
	operator_at(rest(state))
}

pub(super) fn take_until_byte<'a>(
	mut state: ParserState<'a>,
	needle: u8,
) -> Option<(&'a str, ParserState<'a>)> {
	let start = state.pos;
	let bytes = state.input.as_bytes();
	while state.pos < bytes.len() && bytes[state.pos] != needle {
		state.pos += 1;
	}
	if state.pos == bytes.len() {
		return None;
	}
	Some((&state.input[start..state.pos], state))
}

pub(super) fn take_number_literal<'a>(mut state: ParserState<'a>) -> (&'a str, ParserState<'a>) {
	let start = state.pos;
	state = take_ascii_digits(state);
	if peek_byte(&state) == Some(b'.') {
		state = take_ascii_digits(advance(state, 1));
	}
	(&state.input[start..state.pos], state)
}

pub(super) fn take_projection_token<'a>(mut state: ParserState<'a>) -> (&'a str, ParserState<'a>) {
	let start = state.pos;
	let bytes = state.input.as_bytes();
	while state.pos < bytes.len()
		&& (bytes[state.pos].is_ascii_alphabetic()
			|| bytes[state.pos] == b'_'
			|| bytes[state.pos] == b'.')
	{
		state.pos += 1;
	}
	(&state.input[start..state.pos], state)
}

pub(super) fn take_alpha_token<'a>(mut state: ParserState<'a>) -> (&'a str, ParserState<'a>) {
	let start = state.pos;
	let bytes = state.input.as_bytes();
	while state.pos < bytes.len() && bytes[state.pos].is_ascii_alphabetic() {
		state.pos += 1;
	}
	(&state.input[start..state.pos], state)
}

pub(super) fn take_projection_segment<'a>(
	mut state: ParserState<'a>,
) -> (&'a str, ParserState<'a>) {
	let start = state.pos;
	let bytes = state.input.as_bytes();
	while state.pos < bytes.len()
		&& (bytes[state.pos].is_ascii_alphanumeric() || bytes[state.pos] == b'_')
	{
		state.pos += 1;
	}
	(&state.input[start..state.pos], state)
}

pub(super) fn take_domain_ident<'a>(mut state: ParserState<'a>) -> (&'a str, ParserState<'a>) {
	let start = state.pos;
	let bytes = state.input.as_bytes();
	while state.pos < bytes.len()
		&& (bytes[state.pos].is_ascii_alphanumeric()
			|| bytes[state.pos] == b'_'
			|| bytes[state.pos] == b':')
	{
		state.pos += 1;
	}
	(&state.input[start..state.pos], state)
}

pub(super) fn take_atom_text<'a>(state: ParserState<'a>) -> (usize, &'a str, ParserState<'a>) {
	let start = state.pos;
	let end = find_atom_end(&state);
	(start, &state.input[start..end], advance_to(state, end))
}

pub(super) fn peek_atom_text<'a>(state: &ParserState<'a>) -> (usize, &'a str) {
	let start = state.pos;
	let end = find_atom_end(state);
	(start, &state.input[start..end])
}

pub(super) fn slice_from<'a>(state: &ParserState<'a>, start: usize) -> &'a str {
	&state.input[start..state.pos]
}

fn advance_to<'a>(mut state: ParserState<'a>, pos: usize) -> ParserState<'a> {
	state.pos = pos;
	state
}

fn find_atom_end(state: &ParserState<'_>) -> usize {
	let bytes = state.input.as_bytes();
	let mut i = state.pos;
	let mut depth: i32 = 0;
	let mut in_string: Option<u8> = None;
	while i < bytes.len() {
		let c = bytes[i];
		if let Some(q) = in_string {
			if c == q {
				in_string = None;
			}
			i += 1;
			continue;
		}
		match c {
			b'\'' | b'"' => {
				in_string = Some(c);
				i += 1;
			}
			b'(' => {
				depth += 1;
				i += 1;
			}
			b')' => {
				if depth == 0 {
					return i;
				}
				depth -= 1;
				i += 1;
			}
			_ => {
				if depth == 0 && state.input.is_char_boundary(i) && boundary_at(state.input, i) {
					return i;
				}
				i += 1;
			}
		}
	}
	i
}

fn boundary_at(input: &str, i: usize) -> bool {
	let rest = &input[i..];
	rest.starts_with(" AND ")
		|| rest.starts_with(" OR ")
		|| rest.starts_with(" => ")
		|| rest.starts_with(" AND\t")
		|| rest.starts_with(" OR\t")
		|| rest.starts_with(" =>\t")
}

fn take_ascii_digits<'a>(mut state: ParserState<'a>) -> ParserState<'a> {
	let bytes = state.input.as_bytes();
	while state.pos < bytes.len() && bytes[state.pos].is_ascii_digit() {
		state.pos += 1;
	}
	state
}
