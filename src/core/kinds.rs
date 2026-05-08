//! Cross-cutting vocabulary used by both `core/` and `lang/`. Kept in
//! `core/` so `core/code_graph.rs` can compute `binding` defaults
//! without depending on `lang/`. `lang/kinds.rs` re-exports these so
//! per-language extractors keep one import path.

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

// --- binding (cross-file linkage role) ------------------------------------
//
// `bind_match` qualifies the JOIN by `(ref.binding ∈ {import, inject})
// × (def.binding ∈ {export, inject})`. `local` and `none` rows are not
// candidates for cross-file linkage.

/// Def-side: addressable from other modules.
pub const BIND_EXPORT: &[u8] = b"export";
/// Def-side: scoped to the current module (private/module/locals/params).
/// Ref-side: pointer inside the current module.
pub const BIND_LOCAL: &[u8] = b"local";
/// Ref-side: static import from another module.
pub const BIND_IMPORT: &[u8] = b"import";
/// Both sides: DI provider (def) or DI consumer (ref). Treated as
/// `export`/`import` for matching, distinguished only by traceability.
pub const BIND_INJECT: &[u8] = b"inject";
/// Both sides: concept does not apply (sections, unresolved reads).
pub const BIND_NONE: &[u8] = b"none";
