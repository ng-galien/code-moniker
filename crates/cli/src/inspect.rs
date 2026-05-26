//! Compatibility re-exports for the former inspect module.
//!
//! New code should use `code_moniker_cli::workspace::index`.

pub use crate::workspace::index::{
	CheckSummary, DefLocation, RefLocation, SessionOptions, SessionStats, ViewFilter,
};
