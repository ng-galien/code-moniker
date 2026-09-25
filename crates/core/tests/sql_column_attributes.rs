use code_moniker_core::core::moniker::MonikerBuilder;
use code_moniker_core::lang::sql;

fn attribute_identities(source: &str) -> Vec<(String, String)> {
	let anchor = MonikerBuilder::new().project(b"test").build();
	let graph = sql::extract(
		"schema.sql",
		source,
		&anchor,
		false,
		&sql::Presets::default(),
	);
	let mut attributes = graph
		.defs()
		.filter(|definition| definition.kind.as_ref() == b"constraint")
		.map(|definition| {
			(
				String::from_utf8_lossy(
					definition
						.moniker
						.as_view()
						.segments()
						.last()
						.expect("constraint segment")
						.name,
				)
				.into_owned(),
				String::from_utf8_lossy(&definition.signature).into_owned(),
			)
		})
		.collect::<Vec<_>>();
	attributes.sort();
	attributes
}

#[test]
fn column_attributes_keep_stable_identities_across_layout_changes() {
	let compact = attribute_identities(
		"CREATE TABLE app.account (id bigint NOT NULL DEFAULT 1, note text NULL);",
	);
	let formatted = attribute_identities(
		r#"CREATE TABLE app.account (
  id bigint
    NOT NULL
    DEFAULT 1,
  note text NULL
);"#,
	);

	assert_eq!(compact, formatted);
	assert!(
		compact
			.iter()
			.any(|(identity, _)| identity == "id.not_null")
	);
	assert!(compact.iter().any(|(identity, _)| identity == "id.default"));
	assert!(compact.iter().any(|(identity, _)| identity == "note.null"));
}

#[test]
fn default_expression_changes_its_structural_signature() {
	let first = attribute_identities("CREATE TABLE app.account (id bigint DEFAULT 1);");
	let second = attribute_identities("CREATE TABLE app.account (id bigint DEFAULT 2);");

	assert_eq!(first[0].0, second[0].0);
	assert_ne!(first[0].1, second[0].1);
}
