use super::{UriConfig, UriError};
use crate::core::kind_registry::{KindId, KindRegistry, PunctClass};
use crate::core::moniker::{Moniker, MonikerBuilder};

/// Parse a URI into a [`Moniker`]. The registry is consulted (and
/// extended via [`KindRegistry::intern`]) so that each segment's kind
/// resolves to a stable [`KindId`].
///
/// `config.scheme` is the expected URI prefix; the four kind names map
/// URI punctuation to [`KindId`] entries in the registry.
pub fn from_uri(
	uri: &str,
	registry: &mut KindRegistry,
	config: &UriConfig<'_>,
) -> Result<Moniker, UriError> {
	let rest = uri
		.strip_prefix(config.scheme)
		.ok_or_else(|| UriError::MissingScheme(config.scheme.to_string()))?;
	let bytes = rest.as_bytes();

	// Project authority: bytes from start up to first '/' or '#'.
	let mut i = 0;
	while i < bytes.len() && bytes[i] != b'/' && bytes[i] != b'#' {
		i += 1;
	}
	if i == 0 {
		return Err(UriError::MissingProject);
	}
	let project = &bytes[..i];
	std::str::from_utf8(project).map_err(|_| UriError::NonUtf8Project)?;

	let mut builder = MonikerBuilder::new();
	builder.project(project);

	// Path segments — each preceded by '/'. Stops at first '#' or end.
	while i < bytes.len() && bytes[i] == b'/' {
		i += 1; // skip '/'
		let (name, end) = read_name(bytes, i, |c| c == b'/' || c == b'#')?;
		let kind = intern_kind(registry, config.path, PunctClass::Path)?;
		builder.segment(kind, &name);
		i = end;
	}

	// Descriptor section — starts at '#'.
	if i < bytes.len() {
		debug_assert_eq!(bytes[i], b'#');
		i += 1; // skip the leading '#'

		while i < bytes.len() {
			let desc_start = i;
			let (name, end) = read_name(bytes, i, |c| matches!(c, b'#' | b'.' | b'('))?;
			i = end;
			if i >= bytes.len() {
				return Err(UriError::EmptyDescriptor(desc_start));
			}

			match bytes[i] {
				b'#' => {
					let kind = intern_kind(registry, config.type_, PunctClass::Type)?;
					builder.segment(kind, &name);
					i += 1;
				}
				b'.' => {
					let kind = intern_kind(registry, config.term, PunctClass::Term)?;
					builder.segment(kind, &name);
					i += 1;
				}
				b'(' => {
					i += 1; // skip '('
					let (arity, paren_end) = read_arity(bytes, i)?;
					i = paren_end;
					if i >= bytes.len() || bytes[i] != b')' {
						return Err(UriError::BadArity(
							String::from_utf8_lossy(&bytes[desc_start..]).into_owned(),
						));
					}
					i += 1; // skip ')'
					if i >= bytes.len() || bytes[i] != b'.' {
						return Err(UriError::BadArity(
							String::from_utf8_lossy(&bytes[desc_start..]).into_owned(),
						));
					}
					i += 1; // skip '.'
					let kind = intern_kind(registry, config.method, PunctClass::Method)?;
					builder.method(kind, &name, arity);
				}
				_ => unreachable!("read_name stops on # . or ("),
			}
		}
	}

	Ok(builder.build())
}

fn intern_kind(
	registry: &mut KindRegistry,
	name: &str,
	punct: PunctClass,
) -> Result<KindId, UriError> {
	registry
		.intern(name, punct)
		.ok_or(UriError::UnknownKind(KindId::INVALID))
}

/// Read a name from `bytes` starting at `start`, stopping when
/// `is_terminator(byte)` returns true on an unquoted byte. Returns the
/// decoded name (backticks unwrapped) and the new cursor position.
fn read_name(
	bytes: &[u8],
	start: usize,
	is_terminator: impl Fn(u8) -> bool,
) -> Result<(Vec<u8>, usize), UriError> {
	if start < bytes.len() && bytes[start] == b'`' {
		// Backtick-quoted. Read until a single closing backtick (a
		// doubled `` is an escape inside).
		let mut i = start + 1;
		let mut out = Vec::new();
		loop {
			if i >= bytes.len() {
				return Err(UriError::UnterminatedBacktick(start));
			}
			if bytes[i] == b'`' {
				if i + 1 < bytes.len() && bytes[i + 1] == b'`' {
					out.push(b'`');
					i += 2;
				} else {
					i += 1;
					break;
				}
			} else {
				out.push(bytes[i]);
				i += 1;
			}
		}
		Ok((out, i))
	} else {
		let mut i = start;
		while i < bytes.len() && !is_terminator(bytes[i]) {
			i += 1;
		}
		Ok((bytes[start..i].to_vec(), i))
	}
}

fn read_arity(bytes: &[u8], start: usize) -> Result<(u16, usize), UriError> {
	let mut i = start;
	while i < bytes.len() && bytes[i].is_ascii_digit() {
		i += 1;
	}
	if i == start {
		// Empty arity — `()` form.
		return Ok((0, i));
	}
	let s = std::str::from_utf8(&bytes[start..i])
		.map_err(|_| UriError::BadArity(String::from_utf8_lossy(&bytes[start..i]).into_owned()))?;
	let arity: u16 = s.parse().map_err(|_| UriError::BadArity(s.to_string()))?;
	Ok((arity, i))
}

#[cfg(test)]
mod tests {
	use super::super::test_helpers::*;
	use super::*;

	#[test]
	fn from_uri_project_only() {
		let mut reg = fresh_registry();
		let m = from_uri("esac://my-app", &mut reg, &default_config()).unwrap();
		assert_eq!(m.as_view().project(), b"my-app");
		assert_eq!(m.as_view().segment_count(), 0);
	}

	#[test]
	fn from_uri_path_only() {
		let mut reg = fresh_registry();
		let m = from_uri(
			"esac://my-app/main/com/acme/Foo",
			&mut reg,
			&default_config(),
		)
		.unwrap();
		let v = m.as_view();
		assert_eq!(v.project(), b"my-app");
		let segs: Vec<_> = v.segments().collect();
		assert_eq!(segs.len(), 4);
		assert_eq!(segs[0].bytes, b"main");
		assert_eq!(segs[3].bytes, b"Foo");
	}

	#[test]
	fn from_uri_method_with_arity() {
		let mut reg = fresh_registry();
		let m = from_uri(
			"esac://app/main#Foo#bar(2).",
			&mut reg,
			&default_config(),
		)
		.unwrap();
		let segs: Vec<_> = m.as_view().segments().collect();
		assert_eq!(segs.len(), 3);
		assert_eq!(segs[0].bytes, b"main");
		assert_eq!(segs[1].bytes, b"Foo");
		assert_eq!(segs[2].bytes, b"bar");
		assert_eq!(segs[2].arity, 2);
	}

	#[test]
	fn from_uri_method_no_arity() {
		let mut reg = fresh_registry();
		let m = from_uri("esac://app#Foo#bar().", &mut reg, &default_config()).unwrap();
		let segs: Vec<_> = m.as_view().segments().collect();
		assert_eq!(segs[1].bytes, b"bar");
		assert_eq!(segs[1].arity, 0);
	}

	#[test]
	fn from_uri_chained_types_and_term() {
		let mut reg = fresh_registry();
		let m = from_uri(
			"esac://app#Outer#Inner#field.",
			&mut reg,
			&default_config(),
		)
		.unwrap();
		let segs: Vec<_> = m.as_view().segments().collect();
		assert_eq!(segs.len(), 3);
		assert_eq!(segs[0].bytes, b"Outer");
		assert_eq!(segs[1].bytes, b"Inner");
		assert_eq!(segs[2].bytes, b"field");
	}

	#[test]
	fn from_uri_backtick_escape() {
		let mut reg = fresh_registry();
		let m = from_uri(
			"esac://app/`util.test.ts`",
			&mut reg,
			&default_config(),
		)
		.unwrap();
		let segs: Vec<_> = m.as_view().segments().collect();
		assert_eq!(segs[0].bytes, b"util.test.ts");
	}

	#[test]
	fn from_uri_doubled_backtick() {
		let mut reg = fresh_registry();
		let m = from_uri(
			"esac://app#`weird``name`#",
			&mut reg,
			&default_config(),
		)
		.unwrap();
		assert_eq!(
			m.as_view().segments().next().unwrap().bytes,
			b"weird`name"
		);
	}

	#[test]
	fn from_uri_rejects_missing_scheme() {
		let mut reg = fresh_registry();
		let err = from_uri("http://app", &mut reg, &default_config()).unwrap_err();
		match err {
			UriError::MissingScheme(expected) => assert_eq!(expected, "esac://"),
			other => panic!("unexpected error: {other:?}"),
		}
	}

	#[test]
	fn from_uri_rejects_missing_project() {
		let mut reg = fresh_registry();
		assert_eq!(
			from_uri("esac:///foo", &mut reg, &default_config()).unwrap_err(),
			UriError::MissingProject
		);
	}

	#[test]
	fn from_uri_rejects_unterminated_backtick() {
		let mut reg = fresh_registry();
		let r = from_uri("esac://app/`unterminated", &mut reg, &default_config());
		assert!(matches!(r.unwrap_err(), UriError::UnterminatedBacktick(_)));
	}
}
