use super::{UriConfig, UriError};
use crate::core::moniker::Moniker;

pub fn to_uri(moniker: &Moniker, config: &UriConfig<'_>) -> Result<String, UriError> {
	let view = moniker.as_view();
	let mut out = String::with_capacity(config.scheme.len() + view.as_bytes().len() + 16);
	out.push_str(config.scheme);
	write_name(&mut out, view.project());

	for seg in view.segments() {
		out.push('/');
		let kind = std::str::from_utf8(seg.kind).map_err(|_| UriError::NonUtf8Segment)?;
		out.push_str(kind);
		out.push(':');
		write_name(&mut out, seg.name);
	}

	Ok(out)
}

fn name_needs_escaping(bytes: &[u8]) -> bool {
	bytes.is_empty()
		|| bytes
			.iter()
			.any(|b| *b == b'/' || *b == b'`' || b.is_ascii_whitespace())
}

fn write_name(out: &mut String, bytes: &[u8]) {
	let s = std::str::from_utf8(bytes).expect("segment names must be UTF-8");
	if !name_needs_escaping(bytes) {
		out.push_str(s);
		return;
	}
	out.push('`');
	for c in s.chars() {
		if c == '`' {
			out.push_str("``");
		} else {
			out.push(c);
		}
	}
	out.push('`');
}

#[cfg(test)]
mod tests {
	use super::super::test_helpers::*;
	use super::*;
	use crate::core::moniker::MonikerBuilder;

	#[test]
	fn to_uri_project_only() {
		let m = MonikerBuilder::new().project(b"my-app").build();
		assert_eq!(
			to_uri(&m, &default_config()).unwrap(),
			"esac+moniker://my-app"
		);
	}

	#[test]
	fn to_uri_path_chain() {
		let m = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"path", b"com")
			.segment(b"path", b"acme")
			.segment(b"class", b"Foo")
			.build();
		assert_eq!(
			to_uri(&m, &default_config()).unwrap(),
			"esac+moniker://my-app/path:main/path:com/path:acme/class:Foo"
		);
	}

	#[test]
	fn to_uri_method_no_arity_in_name() {
		let m = MonikerBuilder::new()
			.project(b"my-app")
			.segment(b"path", b"main")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar()")
			.build();
		assert_eq!(
			to_uri(&m, &default_config()).unwrap(),
			"esac+moniker://my-app/path:main/class:Foo/method:bar()"
		);
	}

	#[test]
	fn to_uri_method_with_arity_in_name() {
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(b"class", b"Foo")
			.segment(b"method", b"bar(2)")
			.build();
		assert_eq!(
			to_uri(&m, &default_config()).unwrap(),
			"esac+moniker://app/class:Foo/method:bar(2)"
		);
	}

	#[test]
	fn to_uri_escapes_slash_in_name() {
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(b"path", b"util/test.ts")
			.build();
		assert_eq!(
			to_uri(&m, &default_config()).unwrap(),
			"esac+moniker://app/path:`util/test.ts`"
		);
	}

	#[test]
	fn to_uri_escapes_backtick() {
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(b"class", b"weird`name")
			.build();
		assert_eq!(
			to_uri(&m, &default_config()).unwrap(),
			"esac+moniker://app/class:`weird``name`"
		);
	}
}
