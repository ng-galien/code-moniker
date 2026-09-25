use code_moniker_core::core::moniker::MonikerBuilder;
use code_moniker_core::lang::sql;

fn check_signature(source: &str) -> Vec<u8> {
	let anchor = MonikerBuilder::new().project(b"test").build();
	let graph = sql::extract(
		"schema.sql",
		source,
		&anchor,
		false,
		&sql::Presets::default(),
	);
	graph
		.defs()
		.find(|definition| definition.kind.as_ref() == b"constraint")
		.expect("check constraint definition")
		.signature
		.to_vec()
}

#[test]
fn check_constraint_signature_contains_its_expression() {
	let positive = check_signature(
		"CREATE TABLE app.invoice (amount integer CONSTRAINT positive_amount CHECK (amount > 0));",
	);
	let non_negative = check_signature(
		"CREATE TABLE app.invoice (amount integer CONSTRAINT positive_amount CHECK (amount >= 0));",
	);

	assert_eq!(positive, b"CONSTRAINT positive_amount CHECK (amount > 0)");
	assert_ne!(positive, non_negative);
}
