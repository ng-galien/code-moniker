//! Cross-language kind vocabulary for `DefRecord.visibility`,
//! `RefRecord.confidence`, and the `binding` column. Per-language
//! modules add their own structural kinds (class/method/...) but
//! lean on these for the consumer-projection vocabulary defined in
//! `references/kinds.md`.
//!
//! Visibility and binding constants live in `core/kinds.rs` so the
//! `core/code_graph.rs::default_*_binding` helpers can reference
//! them without a layer inversion. They are re-exported here so
//! per-language modules import from a single path.

pub use crate::core::kinds::{
	BIND_EXPORT, BIND_IMPORT, BIND_INJECT, BIND_LOCAL, BIND_NONE,
	VIS_MODULE, VIS_NONE, VIS_PACKAGE, VIS_PRIVATE, VIS_PROTECTED, VIS_PUBLIC,
};

// --- structural ----------------------------------------------------------

/// First segment of every language regime. Posted by each extractor's
/// `compute_module_moniker` immediately under the caller's anchor.
/// Short language names align with the `src/lang/<lang>/` directory
/// (`ts`, `rs`, `java`, `python`, `sql`).
pub const LANG: &[u8] = b"lang";

// --- ref confidence -------------------------------------------------------

pub const CONF_EXTERNAL: &[u8] = b"external";
pub const CONF_IMPORTED: &[u8] = b"imported";
pub const CONF_NAME_MATCH: &[u8] = b"name_match";
pub const CONF_LOCAL: &[u8] = b"local";
pub const CONF_RESOLVED: &[u8] = b"resolved";
pub const CONF_UNRESOLVED: &[u8] = b"unresolved";
