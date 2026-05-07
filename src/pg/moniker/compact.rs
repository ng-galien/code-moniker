//! Display-only SCIP-like projection of a moniker.
//!
//! `moniker_compact(m)` returns a human-readable URI in the legacy
//! punct-class form (`esac://app/main#Foo#bar().`) — concise, lossy
//! (kind precision collapses onto four classes), one-way. There is no
//! `text → moniker` parser for this form by design: identity lives in
//! the canonical typed URI (`moniker_in` / `moniker_out`).
//!
//! `match_compact(m, text)` answers a yes/no — it does not construct a
//! moniker, so callers cannot accidentally round-trip through the
//! lossy form.

use pgrx::prelude::*;

use super::moniker;
use crate::pg::registry::DEFAULT_CONFIG;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum PunctClass {
	Path,
	Type,
	Term,
	Method,
}

fn class_for(kind: &[u8]) -> PunctClass {
	match kind {
		b"path" | b"package" | b"srcset" | b"dir" | b"module"
		| b"workspace_app" | b"external_pkg" => PunctClass::Path,
		b"class" | b"interface" | b"type_alias" | b"type" | b"enum"
		| b"struct" | b"trait" | b"record" => PunctClass::Type,
		b"field" | b"variable" | b"const" | b"constant" | b"property"
		| b"term" => PunctClass::Term,
		b"method" | b"function" | b"constructor" | b"ctor" | b"operator" => {
			PunctClass::Method
		}
		_ => PunctClass::Term,
	}
}

/// SCIP-grammar reserved bytes that need backtick-quoting inside a
/// name. `/` and `#` are segment separators; `.` terminates Term and
/// Method descriptors; backtick is the escape character. Parens are
/// not in the set because v2 method names already carry their
/// `()`/`(N)` disambiguator and SCIP-style display expects them
/// rendered raw (`#bar().`).
const RESERVED: &[u8] = b"/#.`";

fn name_needs_escaping(bytes: &[u8]) -> bool {
	bytes.is_empty()
		|| bytes
			.iter()
			.any(|b| RESERVED.contains(b) || b.is_ascii_whitespace())
}

fn push_name(out: &mut String, bytes: &[u8]) {
	let s = std::str::from_utf8(bytes)
		.unwrap_or_else(|_| error!("moniker name must be UTF-8"));
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

/// Base scheme of the compact form, derived by stripping the
/// `+moniker://` profile suffix from the canonical scheme.
fn compact_scheme() -> String {
	let canonical = DEFAULT_CONFIG.scheme;
	let base = canonical.strip_suffix("+moniker://").unwrap_or("esac");
	format!("{base}://")
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_compact(m: moniker) -> String {
	let view = m.view();
	let mut out = String::with_capacity(view.as_bytes().len() + 16);
	out.push_str(&compact_scheme());
	push_name(&mut out, view.project());

	let mut in_descriptor = false;
	for seg in view.segments() {
		match class_for(seg.kind) {
			PunctClass::Path => {
				out.push('/');
				push_name(&mut out, seg.name);
			}
			PunctClass::Type => {
				if !in_descriptor {
					out.push('#');
					in_descriptor = true;
				}
				push_name(&mut out, seg.name);
				out.push('#');
			}
			PunctClass::Term | PunctClass::Method => {
				// v2 method names already carry their `()`/`(N)`
				// disambiguator, so SCIP-method serialization collapses
				// onto the same shape as a term: `name.`.
				if !in_descriptor {
					out.push('#');
					in_descriptor = true;
				}
				push_name(&mut out, seg.name);
				out.push('.');
			}
		}
	}
	out
}

#[pg_extern(immutable, parallel_safe)]
fn match_compact(m: moniker, compact: &str) -> bool {
	moniker_compact(m) == compact
}
