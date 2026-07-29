//! Emits the daemon RPC JSON Schema to stdout. Run with the `schema` feature:
//! `cargo run -p code-moniker-query --features schema --bin export-schema`.

#[derive(serde::Serialize)]
struct VersionedSchema<T> {
	#[serde(flatten)]
	schema: T,
	#[serde(rename = "x-code-moniker-protocol-version")]
	protocol_version: u32,
}

fn main() {
	let schema = schemars::schema_for!(code_moniker_query::DaemonProtocol);
	let schema = VersionedSchema {
		schema,
		protocol_version: code_moniker_query::PROTOCOL_VERSION,
	};
	println!(
		"{}",
		serde_json::to_string_pretty(&schema).expect("serialize schema")
	);
}
