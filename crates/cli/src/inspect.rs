//! Compatibility re-exports for the former inspect module.
//!
//! New code should use `code_moniker_cli::workspace::index`.

pub use crate::workspace::index::{
	CheckSummary, DefLocation, IndexedFile, IndexedRoot, RefLocation, SessionIndex, SessionOptions,
	SessionStats, ViewFilter,
};
