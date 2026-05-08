//! Pure-Rust core of pg_code_moniker.
//!
//! This module owns the type implementations (`moniker`, `code_graph`)
//! and their algebra. It has no dependency on pgrx or PostgreSQL --
//! the [`crate::pg`] module wraps these types for the SQL surface
//! when a `pgN` feature is enabled.
//!
//! Discipline:
//! - No `unsafe` unless it is the only sensible way and is justified
//!   in a comment above the block.
//! - No file > ~600 lines. Split by concern.
//! - Every public item carries a doc comment describing its contract.

pub mod code_graph;
pub mod kinds;
pub mod moniker;
pub mod uri;
