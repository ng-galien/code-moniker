//! pg_code_moniker -- native PostgreSQL types and indexed algebra for
//! code symbol identity (`moniker`) and code graph storage (`code_graph`).
//!
//! # Layout
//!
//! - [`core`]  Pure Rust, no pgrx, no PG dependencies. Testable in
//!             isolation with `cargo test` (no `--features` required).
//! - [`pg`]    pgrx-backed wrappers exposing the SQL surface. Compiled
//!             only when a `pgN` feature is selected.
//! - [`lang`]  Per-language extractors (tree-sitter). Pure Rust, no pgrx.
//!
//! # Phase 1 scope
//!
//! Type `moniker` minimal: byte-compact internal representation (kind
//! registry + segment-encoded path), URI parse/serialize, equality
//! operator. Indexable (`<@`, `@>`, `~`, GiST) lands in later phases.

pub mod build;
pub mod core;
pub mod lang;

#[cfg(any(feature = "pg14", feature = "pg15", feature = "pg16", feature = "pg17"))]
pub mod pg;

#[cfg(any(feature = "pg14", feature = "pg15", feature = "pg16", feature = "pg17"))]
::pgrx::pg_module_magic!();

#[cfg(test)]
pub mod pg_test {
	pub fn setup(_options: Vec<&str>) {}

	pub fn postgresql_conf_options() -> Vec<&'static str> {
		vec![]
	}
}
