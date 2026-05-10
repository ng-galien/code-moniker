use pgrx::prelude::*;

pub mod build;
pub mod code_graph;
pub mod declare;
pub mod extract;
pub mod moniker;
mod registry;
mod util;

#[pg_guard]
pub extern "C-unwind" fn _PG_init() {
	registry::init_gucs();
}

#[pg_extern]
fn pcm_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
