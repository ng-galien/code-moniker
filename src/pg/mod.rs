use pgrx::prelude::*;

pub mod build;
pub mod code_graph;
pub mod extract;
pub mod moniker;
mod registry;

#[pg_extern]
fn pcm_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
