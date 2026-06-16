//! Emits the daemon RPC JSON Schema to stdout. Run with the `schema` feature:
//! `cargo run -p code-moniker-query --features schema --bin export-schema`.

fn main() {
	let schema = schemars::schema_for!(code_moniker_query::DaemonProtocol);
	println!(
		"{}",
		serde_json::to_string_pretty(&schema).expect("serialize schema")
	);
}
