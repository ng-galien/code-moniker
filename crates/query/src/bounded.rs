use std::fmt::{self, Debug, Display, Write as _};

/// A debug representation that stops formatting after `max_chars` Unicode
/// characters instead of materializing the complete value before truncation.
pub struct BoundedDebug<'a, T: ?Sized> {
	value: &'a T,
	max_chars: usize,
}

pub fn bounded_debug<T: Debug + ?Sized>(value: &T, max_chars: usize) -> BoundedDebug<'_, T> {
	BoundedDebug { value, max_chars }
}

impl<T: Debug + ?Sized> Display for BoundedDebug<'_, T> {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mut writer = BoundedWriter {
			inner: formatter,
			remaining: self.max_chars,
			truncated: false,
		};
		let result = write!(&mut writer, "{:?}", self.value);
		let truncated = writer.truncated;
		drop(writer);
		if truncated {
			formatter.write_str("…")
		} else {
			result
		}
	}
}

struct BoundedWriter<'a, 'b> {
	inner: &'a mut fmt::Formatter<'b>,
	remaining: usize,
	truncated: bool,
}

impl fmt::Write for BoundedWriter<'_, '_> {
	fn write_str(&mut self, value: &str) -> fmt::Result {
		let Some((boundary, count)) = char_boundary(value, self.remaining) else {
			self.remaining -= value.chars().count();
			return self.inner.write_str(value);
		};
		self.inner.write_str(&value[..boundary])?;
		self.remaining -= count;
		self.truncated = true;
		Err(fmt::Error)
	}
}

fn char_boundary(value: &str, max_chars: usize) -> Option<(usize, usize)> {
	value
		.char_indices()
		.nth(max_chars)
		.map(|(boundary, _)| (boundary, max_chars))
}

#[cfg(test)]
mod tests {
	use super::bounded_debug;

	#[test]
	fn bounded_debug_stops_on_unicode_character_boundary() {
		assert_eq!(bounded_debug(&"abécd", 4).to_string(), "\"abé…");
		assert_eq!(bounded_debug(&"ab", 8).to_string(), "\"ab\"");
	}
}
