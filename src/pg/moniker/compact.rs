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

#[pg_extern(immutable, parallel_safe)]
fn moniker_compact(m: moniker) -> String {
	let view = m.view();
	let project = std::str::from_utf8(view.project())
		.unwrap_or_else(|_| error!("moniker project must be UTF-8"));
	let mut out = String::with_capacity(view.as_bytes().len() + 16);
	out.push_str("esac://");
	out.push_str(project);

	let mut in_descriptor = false;
	for seg in view.segments() {
		let name = std::str::from_utf8(seg.name)
			.unwrap_or_else(|_| error!("moniker segment name must be UTF-8"));
		match class_for(seg.kind) {
			PunctClass::Path => {
				out.push('/');
				out.push_str(name);
			}
			PunctClass::Type => {
				if !in_descriptor {
					out.push('#');
					in_descriptor = true;
				}
				out.push_str(name);
				out.push('#');
			}
			PunctClass::Term => {
				if !in_descriptor {
					out.push('#');
					in_descriptor = true;
				}
				out.push_str(name);
				out.push('.');
			}
			PunctClass::Method => {
				if !in_descriptor {
					out.push('#');
					in_descriptor = true;
				}
				// v2 method names already carry their `()` or `(N)` disambiguator
				// inside the bytes, so just append SCIP's trailing `.`.
				out.push_str(name);
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
