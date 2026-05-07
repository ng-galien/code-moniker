//! TypeScript-specific kind names.
//!
//! Byte string constants embedded directly in moniker bytes and tagged
//! on defs/refs. Vocabulary mirrors `references/kinds.md`; identity-kind
//! and label-kind happen to share the same constant when they line up.

// --- path / module segments ----------------------------------------------

pub(super) const PATH: &[u8] = b"path";
pub(super) const EXTERNAL_PKG: &[u8] = b"external_pkg";

// --- type-like defs -------------------------------------------------------

pub(super) const CLASS: &[u8] = b"class";
pub(super) const INTERFACE: &[u8] = b"interface";
pub(super) const ENUM: &[u8] = b"enum";
pub(super) const TYPE_ALIAS: &[u8] = b"type_alias";

// --- callable defs --------------------------------------------------------

pub(super) const FUNCTION: &[u8] = b"function";
pub(super) const METHOD: &[u8] = b"method";
pub(super) const CONSTRUCTOR: &[u8] = b"constructor";

// --- term-like defs -------------------------------------------------------

pub(super) const FIELD: &[u8] = b"field";
pub(super) const CONST: &[u8] = b"const";
pub(super) const ENUM_CONSTANT: &[u8] = b"enum_constant";

// --- structural / resource-scoped defs -----------------------------------

pub(super) const SECTION: &[u8] = b"section";
pub(super) const PARAM: &[u8] = b"param";
pub(super) const LOCAL: &[u8] = b"local";

// --- visibility -----------------------------------------------------------

pub(super) const VIS_PUBLIC: &[u8] = b"public";
pub(super) const VIS_PROTECTED: &[u8] = b"protected";
pub(super) const VIS_PRIVATE: &[u8] = b"private";
pub(super) const VIS_MODULE: &[u8] = b"module";

// --- ref confidence -------------------------------------------------------

pub(super) const CONF_EXTERNAL: &[u8] = b"external";
pub(super) const CONF_IMPORTED: &[u8] = b"imported";
pub(super) const CONF_NAME_MATCH: &[u8] = b"name_match";
pub(super) const CONF_LOCAL: &[u8] = b"local";

// --- ref kinds ------------------------------------------------------------

pub(super) const IMPORTS_SYMBOL: &[u8] = b"imports_symbol";
pub(super) const IMPORTS_MODULE: &[u8] = b"imports_module";
pub(super) const REEXPORTS: &[u8] = b"reexports";
pub(super) const CALLS: &[u8] = b"calls";
pub(super) const METHOD_CALL: &[u8] = b"method_call";
pub(super) const INSTANTIATES: &[u8] = b"instantiates";
pub(super) const EXTENDS: &[u8] = b"extends";
pub(super) const IMPLEMENTS: &[u8] = b"implements";
pub(super) const ANNOTATES: &[u8] = b"annotates";
pub(super) const USES_TYPE: &[u8] = b"uses_type";
pub(super) const READS: &[u8] = b"reads";
pub(super) const DI_REGISTER: &[u8] = b"di_register";
