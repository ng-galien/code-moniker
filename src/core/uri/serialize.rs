use super::{UriConfig, UriError};
use crate::core::kind_registry::{KindRegistry, PunctClass};
use crate::core::moniker::Moniker;

/// Serialise a [`Moniker`] into its canonical URI representation.
///
/// Each segment's kind must be present in `registry` so its punct class
/// can be looked up. `config.scheme` is used as the URI prefix.
pub fn to_uri(
	moniker: &Moniker,
	registry: &KindRegistry,
	config: &UriConfig<'_>,
) -> Result<String, UriError> {
	let view = moniker.as_view();
	let mut out = String::with_capacity(config.scheme.len() + view.as_bytes().len() + 16);
	out.push_str(config.scheme);
	write_name(&mut out, view.project());

	let mut in_descriptor = false;
	for seg in view.segments() {
		let punct = registry
			.punct_class(seg.kind)
			.ok_or(UriError::UnknownKind(seg.kind))?;

		match punct {
			PunctClass::Path => {
				if in_descriptor {
					return Err(UriError::PathAfterDescriptor);
				}
				out.push('/');
				write_name(&mut out, seg.bytes);
			}
			PunctClass::Type => {
				if !in_descriptor {
					out.push('#');
					in_descriptor = true;
				}
				write_name(&mut out, seg.bytes);
				out.push('#');
			}
			PunctClass::Term => {
				if !in_descriptor {
					out.push('#');
					in_descriptor = true;
				}
				write_name(&mut out, seg.bytes);
				out.push('.');
			}
			PunctClass::Method => {
				if !in_descriptor {
					out.push('#');
					in_descriptor = true;
				}
				write_name(&mut out, seg.bytes);
				if seg.arity == 0 {
					out.push_str("().");
				} else {
					out.push('(');
					out.push_str(&seg.arity.to_string());
					out.push_str(").");
				}
			}
		}
	}

	Ok(out)
}

const RESERVED: &[u8] = b"/#.()`";

fn name_needs_escaping(bytes: &[u8]) -> bool {
	bytes.is_empty()
		|| bytes
			.iter()
			.any(|b| RESERVED.contains(b) || b.is_ascii_whitespace())
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
	use crate::core::kind_registry::KindId;
	use crate::core::moniker::MonikerBuilder;

	#[test]
	fn to_uri_project_only() {
		let m = MonikerBuilder::new().project(b"my-app").build();
		let reg = fresh_registry();
		assert_eq!(to_uri(&m, &reg, &default_config()).unwrap(), "esac://my-app");
	}

	#[test]
	fn to_uri_path_only() {
		let mut reg = fresh_registry();
		let path = reg.intern("path", PunctClass::Path).unwrap();
		let m = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.segment(path, b"com")
			.segment(path, b"acme")
			.segment(path, b"Foo")
			.build();
		assert_eq!(
			to_uri(&m, &reg, &default_config()).unwrap(),
			"esac://my-app/main/com/acme/Foo"
		);
	}

	#[test]
	fn to_uri_type_descriptor() {
		let mut reg = fresh_registry();
		let path = reg.intern("path", PunctClass::Path).unwrap();
		let ty = reg.intern("type", PunctClass::Type).unwrap();
		let m = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.segment(path, b"com")
			.segment(ty, b"Foo")
			.build();
		assert_eq!(
			to_uri(&m, &reg, &default_config()).unwrap(),
			"esac://my-app/main/com#Foo#"
		);
	}

	#[test]
	fn to_uri_method_no_arity() {
		let mut reg = fresh_registry();
		let path = reg.intern("path", PunctClass::Path).unwrap();
		let ty = reg.intern("type", PunctClass::Type).unwrap();
		let method = reg.intern("method", PunctClass::Method).unwrap();
		let m = MonikerBuilder::new()
			.project(b"my-app")
			.segment(path, b"main")
			.segment(ty, b"Foo")
			.method(method, b"bar", 0)
			.build();
		assert_eq!(
			to_uri(&m, &reg, &default_config()).unwrap(),
			"esac://my-app/main#Foo#bar()."
		);
	}

	#[test]
	fn to_uri_method_with_arity() {
		let mut reg = fresh_registry();
		let ty = reg.intern("type", PunctClass::Type).unwrap();
		let method = reg.intern("method", PunctClass::Method).unwrap();
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(ty, b"Foo")
			.method(method, b"bar", 2)
			.build();
		assert_eq!(
			to_uri(&m, &reg, &default_config()).unwrap(),
			"esac://app#Foo#bar(2)."
		);
	}

	#[test]
	fn to_uri_term_descriptor() {
		let mut reg = fresh_registry();
		let ty = reg.intern("type", PunctClass::Type).unwrap();
		let term = reg.intern("term", PunctClass::Term).unwrap();
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(ty, b"Foo")
			.segment(term, b"field")
			.build();
		assert_eq!(
			to_uri(&m, &reg, &default_config()).unwrap(),
			"esac://app#Foo#field."
		);
	}

	#[test]
	fn to_uri_chained_types() {
		let mut reg = fresh_registry();
		let ty = reg.intern("type", PunctClass::Type).unwrap();
		let term = reg.intern("term", PunctClass::Term).unwrap();
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(ty, b"Outer")
			.segment(ty, b"Inner")
			.segment(term, b"field")
			.build();
		assert_eq!(
			to_uri(&m, &reg, &default_config()).unwrap(),
			"esac://app#Outer#Inner#field."
		);
	}

	#[test]
	fn to_uri_rejects_path_after_descriptor() {
		let mut reg = fresh_registry();
		let path = reg.intern("path", PunctClass::Path).unwrap();
		let ty = reg.intern("type", PunctClass::Type).unwrap();
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(ty, b"Foo")
			.segment(path, b"oops")
			.build();
		assert_eq!(
			to_uri(&m, &reg, &default_config()).unwrap_err(),
			UriError::PathAfterDescriptor
		);
	}

	#[test]
	fn to_uri_rejects_unknown_kind() {
		let reg = fresh_registry();
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(KindId::from_raw(99), b"x")
			.build();
		assert!(matches!(
			to_uri(&m, &reg, &default_config()).unwrap_err(),
			UriError::UnknownKind(_)
		));
	}

	#[test]
	fn to_uri_escapes_dots_in_path() {
		let mut reg = fresh_registry();
		let path = reg.intern("path", PunctClass::Path).unwrap();
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(path, b"util.test.ts")
			.build();
		assert_eq!(
			to_uri(&m, &reg, &default_config()).unwrap(),
			"esac://app/`util.test.ts`"
		);
	}

	#[test]
	fn to_uri_escapes_backtick() {
		let mut reg = fresh_registry();
		let ty = reg.intern("type", PunctClass::Type).unwrap();
		let m = MonikerBuilder::new()
			.project(b"app")
			.segment(ty, b"weird`name")
			.build();
		assert_eq!(
			to_uri(&m, &reg, &default_config()).unwrap(),
			"esac://app#`weird``name`#"
		);
	}
}
