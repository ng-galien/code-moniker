//! SQL / PL/pgSQL extractor.
//!
//! Unlike the tree-sitter-backed extractors, this one drives PostgreSQL's
//! own parser via pgrx's `pg_sys` FFI. `pg_parse_query` covers the SQL
//! surface (DDL, top-level calls); `plpgsql_compile_inline` covers
//! procedural function bodies. Both require a live PG backend, so the
//! parser-using paths are exercised through pgTAP rather than pure-Rust
//! tests — the canonicalisation helpers are unit-tested in isolation.

mod body;
mod canonicalize;
mod kinds;
mod refs;
mod scope;
mod walker;

use canonicalize::compute_module_moniker;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

#[derive(Clone, Debug, Default)]
pub struct Presets {
	/// Reserved for caller-supplied schema-coordinate hints (Maven-style
	/// for SQL: project name → external moniker shape). Empty in the
	/// MVP; everything unresolved gets `name_match` confidence.
	pub external_schemas: Vec<String>,
}

/// Extract a `CodeGraph` from SQL/PL-pgSQL source. The caller supplies
/// the project anchor and the URI; the returned graph is rooted at the
/// file-as-module moniker.
///
/// Errors raised by the PG parser are caught per top-level statement:
/// a single broken statement does not abort the module extraction.
pub fn extract(
	uri: &str,
	source: &str,
	anchor: &Moniker,
	deep: bool,
	_presets: &Presets,
) -> CodeGraph {
	let module = compute_module_moniker(anchor, uri);
	let mut graph = CodeGraph::new(module.clone(), kinds::MODULE);
	walker::walk_source(source, &module, deep, &mut graph);
	graph
}
