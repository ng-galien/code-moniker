pub use crate::core::kinds::{
	VIS_MODULE, VIS_NONE, VIS_PACKAGE, VIS_PRIVATE, VIS_PROTECTED, VIS_PUBLIC,
};

pub use crate::core::kinds::{
	KIND_LOCAL as LOCAL,
	KIND_MODULE as MODULE,
	KIND_PARAM as PARAM,
	KIND_SECTION as SECTION,
	REF_ANNOTATES as ANNOTATES,
	REF_CALLS as CALLS,
	REF_DI_REGISTER as DI_REGISTER,
	REF_EXTENDS as EXTENDS,
	REF_IMPLEMENTS as IMPLEMENTS,
	REF_IMPORTS_MODULE as IMPORTS_MODULE,
	REF_IMPORTS_SYMBOL as IMPORTS_SYMBOL,
	REF_INSTANTIATES as INSTANTIATES,
	REF_METHOD_CALL as METHOD_CALL,
	REF_READS as READS,
	REF_REEXPORTS as REEXPORTS,
	REF_USES_TYPE as USES_TYPE,
};

pub const LANG: &[u8] = b"lang";

pub const PATH: &[u8] = b"path";
pub const DIR: &[u8] = b"dir";
pub const EXTERNAL_PKG: &[u8] = b"external_pkg";
pub const PACKAGE: &[u8] = b"package";

pub const CLASS: &[u8] = b"class";
pub const INTERFACE: &[u8] = b"interface";
pub const ENUM: &[u8] = b"enum";
pub const TYPE_ALIAS: &[u8] = b"type_alias";
pub const RECORD: &[u8] = b"record";
pub const ANNOTATION_TYPE: &[u8] = b"annotation_type";

pub const FUNCTION: &[u8] = b"function";
pub const METHOD: &[u8] = b"method";
pub const CONSTRUCTOR: &[u8] = b"constructor";

pub const FIELD: &[u8] = b"field";
pub const CONST: &[u8] = b"const";
pub const ENUM_CONSTANT: &[u8] = b"enum_constant";

pub const CONF_EXTERNAL: &[u8] = b"external";
pub const CONF_IMPORTED: &[u8] = b"imported";
pub const CONF_NAME_MATCH: &[u8] = b"name_match";
pub const CONF_LOCAL: &[u8] = b"local";
pub const CONF_RESOLVED: &[u8] = b"resolved";
pub const CONF_UNRESOLVED: &[u8] = b"unresolved";

pub fn name_confidence_for(is_local: bool, deep: bool) -> Option<&'static [u8]> {
	if is_local {
		if deep { Some(CONF_LOCAL) } else { None }
	} else {
		Some(CONF_NAME_MATCH)
	}
}
