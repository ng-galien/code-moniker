pub const VIS_NONE: &[u8] = b"";
pub const VIS_PUBLIC: &[u8] = b"public";
pub const VIS_PROTECTED: &[u8] = b"protected";
pub const VIS_PACKAGE: &[u8] = b"package";
pub const VIS_PRIVATE: &[u8] = b"private";
pub const VIS_MODULE: &[u8] = b"module";

pub const BIND_EXPORT: &[u8] = b"export";
pub const BIND_LOCAL: &[u8] = b"local";
pub const BIND_IMPORT: &[u8] = b"import";
pub const BIND_INJECT: &[u8] = b"inject";
pub const BIND_NONE: &[u8] = b"none";

pub const KIND_MODULE: &[u8] = b"module";
pub const KIND_SECTION: &[u8] = b"section";
pub const KIND_LOCAL: &[u8] = b"local";
pub const KIND_PARAM: &[u8] = b"param";

pub const REF_IMPORTS_SYMBOL: &[u8] = b"imports_symbol";
pub const REF_IMPORTS_MODULE: &[u8] = b"imports_module";
pub const REF_REEXPORTS: &[u8] = b"reexports";
pub const REF_DI_REGISTER: &[u8] = b"di_register";
pub const REF_CALLS: &[u8] = b"calls";
pub const REF_METHOD_CALL: &[u8] = b"method_call";
pub const REF_READS: &[u8] = b"reads";
pub const REF_USES_TYPE: &[u8] = b"uses_type";
pub const REF_INSTANTIATES: &[u8] = b"instantiates";
pub const REF_EXTENDS: &[u8] = b"extends";
pub const REF_IMPLEMENTS: &[u8] = b"implements";
pub const REF_ANNOTATES: &[u8] = b"annotates";
