//! URI parse / serialize for [`crate::core::moniker::Moniker`], SCIP-inspired format.
//!
//! # Format
//!
//! ```text
//! <scheme><project>(/<path-seg>)*(#<descriptor>)*
//! ```
//!
//! The scheme prefix is configurable via [`UriConfig::scheme`]
//! (default `pcm://`).
//!
//! Descriptors carry their kind via a punctuation suffix:
//!
//! | Punct class | Suffix              | Example          |
//! |-------------|---------------------|------------------|
//! | `Path`      | (separator `/`)     | `…/Foo`          |
//! | `Type`      | `#`                 | `Foo#`           |
//! | `Term`      | `.`                 | `field.`         |
//! | `Method`    | `().` or `(N).`     | `bar().` `bar(2).` |
//!
//! The first descriptor (i.e. the first segment that is not Path)
//! is preceded by a single `#` to mark the path-to-descriptor boundary;
//! subsequent descriptors are concatenated, since each carries its own
//! suffix.
//!
//! # Escaping
//!
//! A segment name with reserved characters (`/`, `#`, `.`, `(`, `)`,
//! backtick, or whitespace) is wrapped in backticks. A literal backtick
//! inside such a name is doubled.

use crate::core::kind_registry::KindId;

mod parse;
mod serialize;

pub use parse::from_uri;
pub use serialize::to_uri;

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum UriError {
	MissingScheme(String),
	MissingProject,
	UnknownKind(KindId),
	PathAfterDescriptor,
	UnterminatedBacktick(usize),
	EmptyDescriptor(usize),
	BadArity(String),
	NonUtf8Project,
}

impl std::fmt::Display for UriError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::MissingScheme(expected) => {
				write!(f, "URI does not start with the expected scheme `{expected}`")
			}
			Self::MissingProject => write!(f, "URI has no project authority"),
			Self::UnknownKind(id) => write!(f, "kind id {} is not in the registry", id.as_u16()),
			Self::PathAfterDescriptor => {
				write!(f, "path-level segment cannot follow a descriptor")
			}
			Self::UnterminatedBacktick(pos) => {
				write!(f, "unterminated backtick-quoted name at byte {pos}")
			}
			Self::EmptyDescriptor(pos) => write!(f, "empty descriptor at byte {pos}"),
			Self::BadArity(s) => write!(f, "malformed arity disambiguator: {s}"),
			Self::NonUtf8Project => write!(f, "project authority must be valid UTF-8"),
		}
	}
}

impl std::error::Error for UriError {}

/// URI configuration: the scheme prefix and the names of the four
/// canonical kinds the parser produces. Callers usually instantiate
/// this once for their project and reuse it.
#[derive(Copy, Clone, Debug)]
pub struct UriConfig<'a> {
	pub scheme: &'a str,
	pub path: &'a str,
	pub type_: &'a str,
	pub term: &'a str,
	pub method: &'a str,
}

impl Default for UriConfig<'_> {
	fn default() -> Self {
		Self {
			scheme: "pcm://",
			path: "path",
			type_: "type",
			term: "term",
			method: "method",
		}
	}
}

#[cfg(test)]
mod test_helpers {
	use super::UriConfig;
	use crate::core::kind_registry::KindRegistry;

	pub fn default_config() -> UriConfig<'static> {
		UriConfig {
			scheme: "esac://",
			..UriConfig::default()
		}
	}

	pub fn fresh_registry() -> KindRegistry {
		KindRegistry::new()
	}
}

#[cfg(test)]
mod tests {
	use super::test_helpers::*;
	use super::*;

	#[test]
	fn roundtrip_simple() {
		let mut reg = fresh_registry();
		let original = "esac://my-app/main/com/acme#Foo#bar(2).";
		let m = from_uri(original, &mut reg, &default_config()).unwrap();
		assert_eq!(to_uri(&m, &reg, &default_config()).unwrap(), original);
	}

	/// Backticks and reserved characters survive a full parse → serialize cycle.
	#[test]
	fn roundtrip_with_escapes() {
		let mut reg = fresh_registry();
		let original = "esac://app/`util.test.ts`#`weird``name`#";
		let m = from_uri(original, &mut reg, &default_config()).unwrap();
		assert_eq!(to_uri(&m, &reg, &default_config()).unwrap(), original);
	}

	#[test]
	fn roundtrip_project_only() {
		let mut reg = fresh_registry();
		let original = "esac://my-app";
		let m = from_uri(original, &mut reg, &default_config()).unwrap();
		assert_eq!(to_uri(&m, &reg, &default_config()).unwrap(), original);
	}
}
