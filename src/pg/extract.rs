//! Per-language extraction entry points exposed to SQL.
//!
//! Each `extract_<lang>` parses source via tree-sitter, walks the AST,
//! and emits a [`code_graph`] rooted under the caller-supplied anchor.

use pgrx::prelude::*;

use crate::pg::code_graph::code_graph;
use crate::pg::moniker::moniker;
use crate::pg::registry::with_registry;

#[pg_extern(immutable, parallel_safe)]
fn extract_typescript(uri: &str, source: &str, anchor: moniker) -> code_graph {
	let core_anchor = anchor.to_core();
	let inner = with_registry(|reg| crate::lang::ts::extract(uri, source, &core_anchor, reg));
	code_graph::from_core(inner)
}
