//! Local-scope tracking for PL/pgSQL bodies. Phase 2 only —
//! BEGIN/END blocks introduce nested scopes, DECLARE binds locals.
//! Empty for the DDL-only phase 1.
