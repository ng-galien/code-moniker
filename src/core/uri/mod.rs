//! ```text
//! <scheme><project>(/<kind>:<name>)*
//! ```
//! `kind` ∈ `[A-Za-z][A-Za-z0-9_]*`. A `name` containing `/`, backtick,
//! or ASCII whitespace is backtick-wrapped; a literal backtick inside is
//! doubled.

mod parse;
mod serialize;

pub use parse::from_uri;
pub use serialize::to_uri;

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum UriError {
	MissingScheme(String),
	MissingProject,
	EmptySegment(usize),
	MissingKindSeparator(usize),
	InvalidKind(String),
	UnterminatedBacktick(usize),
	NonUtf8Project,
	NonUtf8Segment,
}

impl std::fmt::Display for UriError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::MissingScheme(expected) => {
				write!(f, "URI does not start with the expected scheme `{expected}`")
			}
			Self::MissingProject => write!(f, "URI has no project authority"),
			Self::EmptySegment(pos) => write!(f, "empty segment at byte {pos}"),
			Self::MissingKindSeparator(pos) => {
				write!(f, "segment at byte {pos} has no `:` between kind and name")
			}
			Self::InvalidKind(s) => write!(
				f,
				"kind `{s}` is not a plain identifier ([A-Za-z][A-Za-z0-9_]*)"
			),
			Self::UnterminatedBacktick(pos) => {
				write!(f, "unterminated backtick-quoted name at byte {pos}")
			}
			Self::NonUtf8Project => write!(f, "project authority must be valid UTF-8"),
			Self::NonUtf8Segment => write!(f, "segment must be valid UTF-8"),
		}
	}
}

impl std::error::Error for UriError {}

#[derive(Copy, Clone, Debug)]
pub struct UriConfig<'a> {
	pub scheme: &'a str,
}

impl Default for UriConfig<'_> {
	fn default() -> Self {
		Self {
			scheme: "pcm+moniker://",
		}
	}
}

#[cfg(test)]
mod test_helpers {
	use super::UriConfig;

	pub fn default_config() -> UriConfig<'static> {
		UriConfig {
			scheme: "esac+moniker://",
		}
	}
}

#[cfg(test)]
mod tests {
	use super::test_helpers::*;
	use super::*;

	#[test]
	fn roundtrip_simple() {
		let original = "esac+moniker://my-app/path:main/path:com/path:acme/class:Foo/method:bar(2)";
		let m = from_uri(original, &default_config()).unwrap();
		assert_eq!(to_uri(&m, &default_config()).unwrap(), original);
	}

	#[test]
	fn roundtrip_with_escapes() {
		let original = "esac+moniker://app/path:`util/test.ts`/class:`weird``name`";
		let m = from_uri(original, &default_config()).unwrap();
		assert_eq!(to_uri(&m, &default_config()).unwrap(), original);
	}

	#[test]
	fn roundtrip_project_only() {
		let original = "esac+moniker://my-app";
		let m = from_uri(original, &default_config()).unwrap();
		assert_eq!(to_uri(&m, &default_config()).unwrap(), original);
	}

	#[test]
	fn roundtrip_typed_callable_names_with_quoting_chars() {
		use crate::core::moniker::MonikerBuilder;
		let names: &[&[u8]] = &[
			b"foo(int,String)",
			b"f((x: number) => string)",
			b"f(string | null)",
			b"render(Map<String, List<Item>>)",
			b"foo with spaces",
		];
		for name in names {
			let m = MonikerBuilder::new()
				.project(b"app")
				.segment(b"path", b"x")
				.segment(b"function", name)
				.build();
			let s = to_uri(&m, &default_config()).expect("serialize");
			let parsed = from_uri(&s, &default_config())
				.unwrap_or_else(|e| panic!("roundtrip failed on {s:?}: {e}"));
			assert_eq!(parsed, m, "roundtrip mismatch for {s:?}");
		}
	}
}
