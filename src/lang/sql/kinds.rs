pub(super) const MODULE: &[u8] = b"module";
pub(super) const SCHEMA: &[u8] = b"schema";
pub(super) const FUNCTION: &[u8] = b"function";
pub(super) const PROCEDURE: &[u8] = b"function";
pub(super) const TABLE: &[u8] = b"class";
pub(super) const VIEW: &[u8] = b"interface";
pub(super) const INDEX: &[u8] = b"index";
pub(super) const TRIGGER: &[u8] = b"trigger";
pub(super) const PARAM: &[u8] = b"param";
pub(super) const LOCAL: &[u8] = b"local";

pub(super) const REF_CALLS: &[u8] = b"calls";
pub(super) const REF_USES_TYPE: &[u8] = b"uses_type";

pub(super) use crate::lang::kinds::{
	CONF_EXTERNAL, CONF_LOCAL, CONF_NAME_MATCH, CONF_RESOLVED, CONF_UNRESOLVED, VIS_NONE,
};
