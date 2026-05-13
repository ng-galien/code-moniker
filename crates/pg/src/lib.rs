use pgrx::prelude::*;

::pgrx::pg_module_magic!();

pub mod build;
pub mod code_graph;
pub mod declare;
pub mod extract;
pub mod moniker;
mod registry;
mod util;

#[cfg(test)]
pub mod pg_test {
	pub fn setup(_options: Vec<&str>) {}

	pub fn postgresql_conf_options() -> Vec<&'static str> {
		vec![]
	}
}

// code-moniker: ignore[name-snakecase] — postgres dlopens this exact symbol name; the init hook contract is `_PG_init`.
#[pg_guard]
pub extern "C-unwind" fn _PG_init() {
	registry::init_gucs();
}

#[pg_extern]
fn pcm_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
