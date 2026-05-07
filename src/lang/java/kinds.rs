//! Java-specific kind names.

pub(super) use crate::lang::kinds::{
	CONF_EXTERNAL, CONF_IMPORTED, CONF_LOCAL, CONF_NAME_MATCH, CONF_RESOLVED, VIS_PACKAGE,
	VIS_PRIVATE, VIS_PROTECTED, VIS_PUBLIC,
};

// --- module shape --------------------------------------------------------

pub(super) const PACKAGE: &[u8] = b"package";
pub(super) const MODULE: &[u8] = b"module";
pub(super) const EXTERNAL_PKG: &[u8] = b"external_pkg";
pub(super) const PATH: &[u8] = b"path";

// --- type-like defs -------------------------------------------------------

pub(super) const CLASS: &[u8] = b"class";
pub(super) const INTERFACE: &[u8] = b"interface";
pub(super) const ENUM: &[u8] = b"enum";
pub(super) const RECORD: &[u8] = b"record";
pub(super) const ANNOTATION_TYPE: &[u8] = b"annotation_type";

// --- callable defs --------------------------------------------------------

pub(super) const METHOD: &[u8] = b"method";
pub(super) const CONSTRUCTOR: &[u8] = b"constructor";

// --- term-like defs -------------------------------------------------------

pub(super) const FIELD: &[u8] = b"field";
pub(super) const ENUM_CONSTANT: &[u8] = b"enum_constant";

// --- structural / resource-scoped ---------------------------------------

pub(super) const PARAM: &[u8] = b"param";
pub(super) const LOCAL: &[u8] = b"local";
pub(super) const SECTION: &[u8] = b"section";

// --- ref kinds ------------------------------------------------------------

pub(super) const IMPORTS_SYMBOL: &[u8] = b"imports_symbol";
pub(super) const IMPORTS_MODULE: &[u8] = b"imports_module";
pub(super) const CALLS: &[u8] = b"calls";
pub(super) const METHOD_CALL: &[u8] = b"method_call";
pub(super) const INSTANTIATES: &[u8] = b"instantiates";
pub(super) const EXTENDS: &[u8] = b"extends";
pub(super) const IMPLEMENTS: &[u8] = b"implements";
pub(super) const ANNOTATES: &[u8] = b"annotates";
pub(super) const USES_TYPE: &[u8] = b"uses_type";
pub(super) const READS: &[u8] = b"reads";
