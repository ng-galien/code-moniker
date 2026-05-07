//! TypeScript-specific kind names.
//!
//! These are the byte strings the extractor embeds in moniker bytes and
//! attaches to defs/refs. They're a controlled vocabulary — kept here
//! so future TS-specific kinds (interface, type_alias, enum, …) land in
//! one place.

pub(super) const PATH: &[u8] = b"path";
pub(super) const CLASS: &[u8] = b"class";
pub(super) const METHOD: &[u8] = b"method";
pub(super) const FUNCTION: &[u8] = b"function";
pub(super) const IMPORT: &[u8] = b"import";
