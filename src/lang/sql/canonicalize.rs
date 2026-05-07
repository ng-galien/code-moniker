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

pub(super) use crate::lang::callable::{extend_callable_arity, extend_callable_typed, extend_segment};

/// `parent/schema:<schema>` when schema is non-empty; passthrough
/// otherwise. SQL identifiers from PG are stored case-folded by the
/// parser, so we trust the caller to pass them as-is.
pub(super) fn maybe_schema(parent: &Moniker, schema: &[u8]) -> Moniker {
	if schema.is_empty() {
		parent.clone()
	} else {
		extend_segment(parent, kinds::SCHEMA, schema)
	}
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
