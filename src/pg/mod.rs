//! pgrx wrappers exposing the SQL surface. Compiled only when a `pgN`
//! feature is selected.

use pgrx::prelude::*;

pub mod build;
pub mod code_graph;
pub mod extract;
pub mod moniker;
mod registry;

/// Smoke-test entry point. Returns the crate version as `text`.
#[pg_extern]
fn pcm_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
