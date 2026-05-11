#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "cli")]
pub mod cli;
pub mod core;
pub mod declare;
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
