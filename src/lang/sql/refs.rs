//! Reference emitters for SQL/PL-pgSQL. Populated alongside the walker
//! in phase 1 (top-level calls, type uses) and phase 2 (intra-body
//! calls / reads from `plpgsql_compile_inline`).
