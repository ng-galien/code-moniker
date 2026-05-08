//! Cross-language kind vocabulary for `DefRecord.visibility` and
//! `RefRecord.confidence`. Per-language modules add their own
//! structural kinds (class/method/...) but lean on these for the
//! consumer-projection vocabulary defined in
//! `references/kinds.md`.

// --- structural ----------------------------------------------------------

/// First segment of every language regime. Posted by each extractor's
/// `compute_module_moniker` immediately under the caller's anchor.
/// Short language names align with the `src/lang/<lang>/` directory
/// (`ts`, `rs`, `java`, `python`, `sql`).
pub const LANG: &[u8] = b"lang";

// --- visibility -----------------------------------------------------------

/// Sentinel for "concept does not apply" (locals, params, sections,
/// anonymous callbacks). Empty bytes so consumers can filter on
/// `visibility IS NULL` after the SQL surface.
pub const VIS_NONE: &[u8] = b"";
pub const VIS_PUBLIC: &[u8] = b"public";
pub const VIS_PROTECTED: &[u8] = b"protected";
pub const VIS_PACKAGE: &[u8] = b"package";
pub const VIS_PRIVATE: &[u8] = b"private";
pub const VIS_MODULE: &[u8] = b"module";

// --- ref confidence -------------------------------------------------------

pub const CONF_EXTERNAL: &[u8] = b"external";
pub const CONF_IMPORTED: &[u8] = b"imported";
pub const CONF_NAME_MATCH: &[u8] = b"name_match";
pub const CONF_LOCAL: &[u8] = b"local";
pub const CONF_RESOLVED: &[u8] = b"resolved";
pub const CONF_UNRESOLVED: &[u8] = b"unresolved";
