// SQL maps several constructs onto the cross-language moniker kind vocabulary:
// `CREATE TABLE` -> kind:class, `CREATE VIEW` -> kind:interface,
// `CREATE PROCEDURE` -> kind:function. The aliases preserve SQL-side semantics
// in walker code while emitting the canonical moniker bytes.
pub(super) use crate::lang::kinds::CLASS as TABLE;
pub(super) use crate::lang::kinds::FUNCTION as PROCEDURE;
pub(super) use crate::lang::kinds::INTERFACE as VIEW;

pub(super) use crate::lang::kinds::{
	CALLS as REF_CALLS, CONF_UNRESOLVED, FUNCTION, LOCAL, MODULE, PARAM,
	USES_TYPE as REF_USES_TYPE, VIS_NONE,
};

pub(super) const SCHEMA: &[u8] = b"schema";
pub(super) const INDEX: &[u8] = b"index";
