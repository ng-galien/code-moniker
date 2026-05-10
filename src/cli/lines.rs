/// 1-indexed line range for the byte slice `[start, end)` in `source`.
/// `end_line` is the line of the LAST byte (`end - 1`) — a slice that ends
/// just past a `\n` does not yet "touch" the next line. Empty / out-of-bounds
/// ranges collapse to a single line at the start position. Convention matches
/// what every IDE shows the user.
pub fn line_range(source: &str, start: u32, end: u32) -> (u32, u32) {
	let bytes = source.as_bytes();
	let s = (start as usize).min(bytes.len());
	let e = (end as usize).min(bytes.len()).max(s);
	let start_line = 1 + bytes[..s].iter().filter(|b| **b == b'\n').count() as u32;
	let last = if e > s { e - 1 } else { s };
	let end_line = 1 + bytes[..last.min(bytes.len())]
		.iter()
		.filter(|b| **b == b'\n')
		.count() as u32;
	(start_line, end_line)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn single_line_def_is_one_line() {
		let s = "alpha\nbeta\ngamma\n";
		assert_eq!(line_range(s, 0, 5), (1, 1));
	}

	#[test]
	fn multi_line_def_spans_lines_inclusive() {
		let s = "alpha\nbeta\ngamma\n";
		assert_eq!(line_range(s, 0, 11), (1, 2));
	}

	#[test]
	fn def_starting_on_line_three() {
		let s = "a\nb\nc\nd\n";
		assert_eq!(line_range(s, 4, 5), (3, 3));
	}

	#[test]
	fn def_ending_at_eof_without_newline() {
		let s = "a\nb\nc";
		assert_eq!(line_range(s, 4, 5), (3, 3));
	}

	#[test]
	fn out_of_bounds_clamps_safely() {
		let s = "a\nb\n";
		assert_eq!(line_range(s, 100, 200), (3, 3));
	}

	#[test]
	fn end_before_start_collapses_to_start() {
		let s = "a\nb\nc\n";
		assert_eq!(line_range(s, 4, 2), (3, 3));
	}
}
