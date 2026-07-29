pub(super) use crate::lang::kinds::{
	CALLS as REF_CALLS, COLUMN, COMMENT, CONF_EXTERNAL, CONF_NAME_MATCH, CONSTRAINT, FUNCTION,
	MODULE, PATH, READS as REF_READS, REFERENCES as REF_REFERENCES, TABLE, TRIGGER, TYPE,
	USES_TYPE, VIEW, VIS_NONE, WRITES as REF_WRITES,
};

pub(super) const SCHEMA: &[u8] = b"schema";
pub(super) const PROCEDURE: &[u8] = b"procedure";
