use code_moniker_core::core::moniker::MonikerBuilder;
use code_moniker_core::lang::sql;

fn definitions(source: &str) -> Vec<(String, String, String)> {
	let anchor = MonikerBuilder::new().project(b"test").build();
	let graph = sql::extract(
		"schema.sql",
		source,
		&anchor,
		false,
		&sql::Presets::default(),
	);
	let mut definitions = graph
		.defs()
		.map(|definition| {
			(
				String::from_utf8_lossy(definition.kind.as_ref()).into_owned(),
				String::from_utf8_lossy(
					definition
						.moniker
						.as_view()
						.segments()
						.last()
						.expect("definition segment")
						.name,
				)
				.into_owned(),
				String::from_utf8_lossy(&definition.signature).into_owned(),
			)
		})
		.collect::<Vec<_>>();
	definitions.sort();
	definitions
}

#[test]
fn alter_table_add_column_updates_the_virtual_schema() {
	let schema = definitions(
		r#"CREATE SCHEMA hall;
CREATE TABLE hall.ballots (ballot_id UUID PRIMARY KEY);
ALTER TABLE hall.ballots
  ADD COLUMN result_preview UUID UNIQUE REFERENCES hall.council_decisions(council_decision_id);"#,
	);

	assert!(schema.iter().any(|(kind, name, signature)| {
		kind == "column" && name == "result_preview" && signature == "uuid"
	}));
}

#[test]
fn drop_function_removes_only_the_exact_overload() {
	let schema = definitions(
		r#"CREATE SCHEMA hall_api;
CREATE FUNCTION hall_api.freeze(target UUID, expected INTEGER, content BYTEA)
RETURNS VOID LANGUAGE sql AS $$ SELECT $$;
CREATE FUNCTION hall_api.freeze(target UUID)
RETURNS VOID LANGUAGE sql AS $$ SELECT $$;
DROP FUNCTION hall_api.freeze(UUID, INTEGER, BYTEA);"#,
	);
	let functions = schema
		.iter()
		.filter(|(kind, _, _)| kind == "function")
		.collect::<Vec<_>>();

	assert_eq!(functions.len(), 1);
	assert_eq!(functions[0].1, "freeze(target:uuid)");
}
