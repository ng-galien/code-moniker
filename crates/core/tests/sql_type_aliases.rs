use code_moniker_core::core::moniker::MonikerBuilder;
use code_moniker_core::lang::sql;

fn function_signature(source: &str) -> Vec<u8> {
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
		.find(|definition| definition.kind.as_ref() == b"function")
		.expect("function definition")
		.signature
		.to_vec()
}

#[test]
fn postgres_type_aliases_match_catalog_type_names() {
	let aliases = function_signature(
		"CREATE FUNCTION hall.sample(moment timestamptz, label varchar, active bool) RETURNS int AS $$ SELECT 1 $$ LANGUAGE sql;",
	);
	let catalog = function_signature(
		"CREATE FUNCTION hall.sample(moment timestamp with time zone, label character varying, active boolean) RETURNS integer AS $$ SELECT 1 $$ LANGUAGE sql;",
	);

	assert_eq!(aliases, catalog);
}

#[test]
fn unqualified_custom_types_inherit_the_callable_schema() {
	let manifest = function_signature(
		"CREATE FUNCTION hall.count_choice(choice vote_choice) RETURNS int AS $$ SELECT 1 $$ LANGUAGE sql;",
	);
	let catalog = function_signature(
		"CREATE FUNCTION hall.count_choice(choice hall.vote_choice) RETURNS integer AS $$ SELECT 1 $$ LANGUAGE sql;",
	);

	assert_eq!(manifest, catalog);
}
