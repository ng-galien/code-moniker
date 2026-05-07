//! Module-moniker construction and segment extension for SQL/PL-pgSQL.
//!
//! Schema names are NOT part of the module moniker. They live inside
//! function/table/view monikers as a `#schema:<name>` segment so that
//! `public.foo` and `esac.foo` co-existing in the same file produce
//! distinct identities.

use crate::core::moniker::{Moniker, MonikerBuilder};

use super::kinds;

/// Build the file-as-module moniker. Only the path segments coming from
/// the URI matter. Strips the trailing `.sql`/`.psql`/`.pgsql` extension.
pub(super) fn compute_module_moniker(anchor: &Moniker, uri: &str) -> Moniker {
	let (dirs, stem) = split_uri(uri);
	let mut b = MonikerBuilder::from_view(anchor.as_view());
	for d in dirs {
		b.segment(b"dir", d.as_bytes());
	}
	b.segment(kinds::MODULE, stem.as_bytes());
	b.build()
}

pub(super) fn split_uri(uri: &str) -> (Vec<&str>, &str) {
	let pieces: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();
	let (last, dirs) = pieces.split_last().map(|(l, ds)| (*l, ds)).unwrap_or((uri, &[][..]));
	(dirs.to_vec(), file_stem(last))
}

pub(super) fn file_stem(name: &str) -> &str {
	for ext in [".sql", ".psql", ".pgsql"] {
		if let Some(s) = name.strip_suffix(ext) {
			return s;
		}
	}
	name
}

pub(super) fn extend_segment(parent: &Moniker, kind: &[u8], name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::from_view(parent.as_view());
	b.segment(kind, name);
	b.build()
}

/// `<parent>#schema:<schema>` when schema is non-empty; passthrough
/// otherwise. SQL identifiers from PG are stored case-folded by the
/// parser, so we trust the caller to pass them as-is.
pub(super) fn maybe_schema(parent: &Moniker, schema: &[u8]) -> Moniker {
	if schema.is_empty() {
		parent.clone()
	} else {
		extend_segment(parent, kinds::SCHEMA, schema)
	}
}

/// Callable moniker for definitions where parameter types are
/// statically known. PostgreSQL allows same-name same-arity overloads
/// (`min(int)` vs `min(text)`), so arity alone collides — full types
/// in the moniker are load-bearing for SQL identity.
///
/// Segment name: `name(t1,t2,...)` or `name()` for arity 0.
pub(super) fn extend_callable_typed(
	parent: &Moniker,
	kind: &[u8],
	name: &[u8],
	arg_types: &[Vec<u8>],
) -> Moniker {
	extend_segment(parent, kind, &callable_segment_typed(name, arg_types))
}

pub(super) fn callable_segment_typed(name: &[u8], arg_types: &[Vec<u8>]) -> Vec<u8> {
	let body_len: usize = arg_types.iter().map(|t| t.len() + 1).sum();
	let mut full = Vec::with_capacity(name.len() + 2 + body_len);
	full.extend_from_slice(name);
	full.push(b'(');
	for (i, t) in arg_types.iter().enumerate() {
		if i > 0 {
			full.push(b',');
		}
		full.extend_from_slice(t);
	}
	full.push(b')');
	full
}

/// Callable moniker for call sites where only arity is statically
/// known (raw_parser does not resolve argument types). The resulting
/// ref target won't directly match a typed def via `=`; consumers
/// match through a name-and-arity projection or through best-effort
/// type inference layered on top.
///
/// Segment name: `name()` for arity 0, `name(N)` otherwise.
pub(super) fn extend_callable_arity(
	parent: &Moniker,
	kind: &[u8],
	name: &[u8],
	arity: u16,
) -> Moniker {
	extend_segment(parent, kind, &callable_segment_arity(name, arity))
}

pub(super) fn callable_segment_arity(name: &[u8], arity: u16) -> Vec<u8> {
	let mut full = Vec::with_capacity(name.len() + 6);
	full.extend_from_slice(name);
	full.push(b'(');
	if arity != 0 {
		full.extend_from_slice(arity.to_string().as_bytes());
	}
	full.push(b')');
	full
}

#[cfg(test)]
mod tests {
	use super::*;

	fn anchor() -> Moniker {
		MonikerBuilder::new().project(b"app").build()
	}

	#[test]
	fn file_stem_strips_known_sql_extensions() {
		assert_eq!(file_stem("create_plan.sql"), "create_plan");
		assert_eq!(file_stem("create_plan.psql"), "create_plan");
		assert_eq!(file_stem("create_plan.pgsql"), "create_plan");
		assert_eq!(file_stem("noext"), "noext");
	}

	#[test]
	fn module_moniker_path_segments_become_dirs() {
		let m = compute_module_moniker(&anchor(), "db/functions/plan/create_plan.sql");
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"dir", b"db")
			.segment(b"dir", b"functions")
			.segment(b"dir", b"plan")
			.segment(b"module", b"create_plan")
			.build();
		assert_eq!(m, expected);
	}

	#[test]
	fn module_moniker_bare_filename_emits_module_only() {
		let m = compute_module_moniker(&anchor(), "create_plan.sql");
		let expected = MonikerBuilder::new()
			.project(b"app")
			.segment(b"module", b"create_plan")
			.build();
		assert_eq!(m, expected);
	}

	#[test]
	fn callable_segment_typed_arity_zero_drops_types() {
		assert_eq!(callable_segment_typed(b"foo", &[]), b"foo()".to_vec());
	}

	#[test]
	fn callable_segment_typed_joins_types_with_commas() {
		let types = vec![b"int4".to_vec(), b"text".to_vec()];
		assert_eq!(
			callable_segment_typed(b"foo", &types),
			b"foo(int4,text)".to_vec()
		);
	}

	#[test]
	fn callable_segment_arity_zero_drops_number() {
		assert_eq!(callable_segment_arity(b"foo", 0), b"foo()".to_vec());
	}

	#[test]
	fn callable_segment_arity_keeps_count() {
		assert_eq!(callable_segment_arity(b"foo", 2), b"foo(2)".to_vec());
	}

	#[test]
	fn maybe_schema_appends_when_present() {
		let parent = MonikerBuilder::from_view(anchor().as_view())
			.segment(b"module", b"m")
			.build();
		let with_schema = maybe_schema(&parent, b"public");
		let last = with_schema.as_view().segments().last().unwrap();
		assert_eq!(last.kind, b"schema");
		assert_eq!(last.name, b"public");
	}

	#[test]
	fn maybe_schema_passthrough_when_empty() {
		let parent = MonikerBuilder::from_view(anchor().as_view())
			.segment(b"module", b"m")
			.build();
		assert_eq!(maybe_schema(&parent, b""), parent);
	}
}
