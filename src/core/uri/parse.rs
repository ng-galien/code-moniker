use super::{UriConfig, UriError};
use crate::core::moniker::{Moniker, MonikerBuilder};

pub fn from_uri(uri: &str, config: &UriConfig<'_>) -> Result<Moniker, UriError> {
	let rest = uri
		.strip_prefix(config.scheme)
		.ok_or_else(|| UriError::MissingScheme(config.scheme.to_string()))?;
	let bytes = rest.as_bytes();

	let mut i = 0;
	let (project, project_end) = read_name(bytes, i, |c| c == b'/')?;
	if project.is_empty() {
		return Err(UriError::MissingProject);
	}
	std::str::from_utf8(&project).map_err(|_| UriError::NonUtf8Project)?;
	i = project_end;

	let mut builder = MonikerBuilder::new();
	builder.project(&project);

	while i < bytes.len() {
		debug_assert_eq!(bytes[i], b'/');
		i += 1;
		let seg_start = i;
		let (kind, kind_end) = read_kind(bytes, i, seg_start)?;
		i = kind_end;
		if i >= bytes.len() || bytes[i] != b':' {
			return Err(UriError::MissingKindSeparator(seg_start));
		}
		i += 1;
		let (name, name_end) = read_name(bytes, i, |c| c == b'/')?;
		if name.is_empty() && kind.is_empty() {
			return Err(UriError::EmptySegment(seg_start));
		}
		std::str::from_utf8(&name).map_err(|_| UriError::NonUtf8Segment)?;
		builder.segment(&kind, &name);
		i = name_end;
	}

	Ok(builder.build())
}

fn read_kind(bytes: &[u8], start: usize, seg_start: usize) -> Result<(Vec<u8>, usize), UriError> {
	let mut i = start;
	while i < bytes.len() && bytes[i] != b':' && bytes[i] != b'/' {
		i += 1;
	}
	let kind = &bytes[start..i];
	if kind.is_empty() {
		return Err(UriError::EmptySegment(seg_start));
	}
	if !is_plain_identifier(kind) {
		return Err(UriError::InvalidKind(
			String::from_utf8_lossy(kind).into_owned(),
		));
	}
	Ok((kind.to_vec(), i))
}

fn is_plain_identifier(bytes: &[u8]) -> bool {
	if bytes.is_empty() {
		return false;
	}
	let first = bytes[0];
	if !(first.is_ascii_alphabetic()) {
		return false;
	}
	bytes[1..]
		.iter()
		.all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn read_name(
	bytes: &[u8],
	start: usize,
	is_terminator: impl Fn(u8) -> bool,
) -> Result<(Vec<u8>, usize), UriError> {
	if start < bytes.len() && bytes[start] == b'`' {
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
		if i < bytes.len() && !is_terminator(bytes[i]) {
			return Err(UriError::TrailingAfterBacktick(start));
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

#[cfg(test)]
mod tests {
	use super::super::test_helpers::*;
	use super::*;

	#[test]
	fn from_uri_project_only() {
		let m = from_uri("esac+moniker://my-app", &default_config()).unwrap();
		assert_eq!(m.as_view().project(), b"my-app");
		assert_eq!(m.as_view().segment_count(), 0);
	}

	#[test]
	fn from_uri_path_chain() {
		let m = from_uri(
			"esac+moniker://my-app/path:main/path:com/path:acme/class:Foo",
			&default_config(),
		)
		.unwrap();
		let v = m.as_view();
		let segs: Vec<_> = v.segments().collect();
		assert_eq!(segs.len(), 4);
		assert_eq!(segs[0].kind, b"path");
		assert_eq!(segs[0].name, b"main");
		assert_eq!(segs[3].kind, b"class");
		assert_eq!(segs[3].name, b"Foo");
	}

	#[test]
	fn from_uri_method_with_arity_in_name() {
		let m = from_uri(
			"esac+moniker://app/class:Foo/method:bar(2)",
			&default_config(),
		)
		.unwrap();
		let segs: Vec<_> = m.as_view().segments().collect();
		assert_eq!(segs[1].kind, b"method");
		assert_eq!(segs[1].name, b"bar(2)");
	}

	#[test]
	fn from_uri_typed_signature_in_name() {
		let m = from_uri(
			"esac+moniker://app/class:UserService/method:findById(String)",
			&default_config(),
		)
		.unwrap();
		let segs: Vec<_> = m.as_view().segments().collect();
		assert_eq!(segs[1].name, b"findById(String)");
	}

	#[test]
	fn from_uri_backtick_name() {
		let m = from_uri("esac+moniker://app/path:`util/test.ts`", &default_config()).unwrap();
		let segs: Vec<_> = m.as_view().segments().collect();
		assert_eq!(segs[0].name, b"util/test.ts");
	}

	#[test]
	fn from_uri_doubled_backtick() {
		let m = from_uri("esac+moniker://app/class:`weird``name`", &default_config()).unwrap();
		assert_eq!(m.as_view().segments().next().unwrap().name, b"weird`name");
	}

	#[test]
	fn from_uri_rejects_missing_scheme() {
		let err = from_uri("esac://app", &default_config()).unwrap_err();
		match err {
			UriError::MissingScheme(expected) => assert_eq!(expected, "esac+moniker://"),
			other => panic!("unexpected error: {other:?}"),
		}
	}

	#[test]
	fn from_uri_rejects_missing_project() {
		assert_eq!(
			from_uri("esac+moniker:///path:foo", &default_config()).unwrap_err(),
			UriError::MissingProject
		);
	}

	#[test]
	fn from_uri_rejects_missing_kind_separator() {
		let err = from_uri("esac+moniker://app/just_a_name", &default_config()).unwrap_err();
		assert!(matches!(err, UriError::MissingKindSeparator(_)));
	}

	#[test]
	fn from_uri_rejects_invalid_kind_starting_with_digit() {
		let err = from_uri("esac+moniker://app/9bad:name", &default_config()).unwrap_err();
		assert!(matches!(err, UriError::InvalidKind(_)));
	}

	#[test]
	fn from_uri_rejects_unterminated_backtick() {
		let r = from_uri("esac+moniker://app/path:`unterminated", &default_config());
		assert!(matches!(r.unwrap_err(), UriError::UnterminatedBacktick(_)));
	}

	#[test]
	fn from_uri_rejects_trailing_data_after_backtick() {
		let r = from_uri("esac+moniker://`x`A", &default_config());
		assert!(matches!(r.unwrap_err(), UriError::TrailingAfterBacktick(_)));
	}

	use proptest::prelude::*;

	proptest! {
		#![proptest_config(ProptestConfig {
			cases: 256,
			..ProptestConfig::default()
		})]

		#[test]
		fn from_uri_never_panics(input in ".{0,512}") {
			let _ = from_uri(&input, &default_config());
		}

		#[test]
		fn from_uri_with_scheme_never_panics(suffix in ".{0,512}") {
			let s = format!("esac+moniker://{suffix}");
			let _ = from_uri(&s, &default_config());
		}

		#[test]
		fn from_uri_lossy_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
			let s = String::from_utf8_lossy(&bytes);
			let _ = from_uri(&s, &default_config());
		}
	}
}
