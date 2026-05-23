pub(super) struct Parser<'a> {
	pub(super) input: &'a str,
	pub(super) pos: usize,
	pub(super) scheme: &'a str,
	pub(super) allowed_kinds: &'a [&'a str],
	pub(super) raw: &'a str,
}

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

impl<'a> Parser<'a> {
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
		}
	}

	pub(super) fn skip_ws(&mut self) {
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
			self.pos += 1;
		}
	}

	pub(super) fn peek_byte(&self) -> Option<u8> {
		self.input.as_bytes().get(self.pos).copied()
	}

	pub(super) fn take_until_byte(&mut self, needle: u8) -> Option<&'a str> {
		let start = self.pos;
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len() && bytes[self.pos] != needle {
			self.pos += 1;
		}
		if self.pos == bytes.len() {
			return None;
		}
		Some(&self.input[start..self.pos])
	}

	pub(super) fn take_number_literal(&mut self) -> &'a str {
		let start = self.pos;
		self.take_ascii_digits();
		if self.peek_byte() == Some(b'.') {
			self.pos += 1;
			self.take_ascii_digits();
		}
		&self.input[start..self.pos]
	}

	pub(super) fn take_projection_token(&mut self) -> &'a str {
		let start = self.pos;
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len()
			&& (bytes[self.pos].is_ascii_alphabetic()
				|| bytes[self.pos] == b'_'
				|| bytes[self.pos] == b'.')
		{
			self.pos += 1;
		}
		&self.input[start..self.pos]
	}

	pub(super) fn take_alpha_token(&mut self) -> &'a str {
		let start = self.pos;
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len() && bytes[self.pos].is_ascii_alphabetic() {
			self.pos += 1;
		}
		&self.input[start..self.pos]
	}

	pub(super) fn take_projection_segment(&mut self) -> &'a str {
		let start = self.pos;
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len()
			&& (bytes[self.pos].is_ascii_alphanumeric() || bytes[self.pos] == b'_')
		{
			self.pos += 1;
		}
		&self.input[start..self.pos]
	}

	pub(super) fn take_domain_ident(&mut self) -> &'a str {
		let start = self.pos;
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len()
			&& (bytes[self.pos].is_ascii_alphanumeric()
				|| bytes[self.pos] == b'_'
				|| bytes[self.pos] == b':')
		{
			self.pos += 1;
		}
		&self.input[start..self.pos]
	}

	pub(super) fn find_atom_end(&self) -> usize {
		let bytes = self.input.as_bytes();
		let mut i = self.pos;
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
					if depth == 0 && self.boundary_at(i) {
						return i;
					}
					i += 1;
				}
			}
		}
		i
	}

	pub(super) fn eat_op(&self) -> Option<(&'static str, usize)> {
		operator_at(&self.input[self.pos..])
	}

	pub(super) fn eat_keyword(&mut self, kw: &str) -> bool {
		let rest = &self.input[self.pos..];
		if let Some(after) = rest.strip_prefix(kw) {
			let next_ok = after.is_empty()
				|| after.starts_with(|c: char| c.is_ascii_whitespace())
				|| after.starts_with('(');
			if next_ok {
				self.pos += kw.len();
				return true;
			}
		}
		false
	}

	fn boundary_at(&self, i: usize) -> bool {
		let rest = &self.input[i..];
		rest.starts_with(" AND ")
			|| rest.starts_with(" OR ")
			|| rest.starts_with(" => ")
			|| rest.starts_with(" AND\t")
			|| rest.starts_with(" OR\t")
			|| rest.starts_with(" =>\t")
	}

	fn take_ascii_digits(&mut self) {
		let bytes = self.input.as_bytes();
		while self.pos < bytes.len() && bytes[self.pos].is_ascii_digit() {
			self.pos += 1;
		}
	}
}
