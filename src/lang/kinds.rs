pub use crate::core::kinds::{
	BIND_EXPORT, BIND_IMPORT, BIND_INJECT, BIND_LOCAL, BIND_NONE,
	VIS_MODULE, VIS_NONE, VIS_PACKAGE, VIS_PRIVATE, VIS_PROTECTED, VIS_PUBLIC,
};

pub const LANG: &[u8] = b"lang";

pub const CONF_EXTERNAL: &[u8] = b"external";
pub const CONF_IMPORTED: &[u8] = b"imported";
pub const CONF_NAME_MATCH: &[u8] = b"name_match";
pub const CONF_LOCAL: &[u8] = b"local";
pub const CONF_RESOLVED: &[u8] = b"resolved";
pub const CONF_UNRESOLVED: &[u8] = b"unresolved";
