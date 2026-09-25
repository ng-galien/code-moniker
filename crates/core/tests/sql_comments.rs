use code_moniker_core::core::moniker::MonikerBuilder;
use code_moniker_core::lang::{LangExtractor, sql};

#[test]
fn comment_on_column_is_a_stable_column_annotation() {
	let source = r#"CREATE TABLE hall.contact (
  id uuid PRIMARY KEY,
  contact_id uuid
);
COMMENT ON COLUMN hall.contact.contact_id IS '@hall renamed-from council_member_id';"#;
	let parsed = sql::Lang::parse("schema.sql", source);
	assert!(!parsed.primary().root_node().has_error());
	let anchor = MonikerBuilder::new().project(b"test").build();
	let graph = sql::extract(
		"schema.sql",
		source,
		&anchor,
		false,
		&sql::Presets::default(),
	);
	let comment = graph
		.defs()
		.find(|definition| definition.kind.as_ref() == b"comment")
		.expect("column comment definition");
	let segments = comment.moniker.as_view().segments().collect::<Vec<_>>();
	assert_eq!(segments[segments.len() - 2].kind, b"column");
	assert_eq!(segments[segments.len() - 2].name, b"contact_id");
	assert_eq!(segments.last().unwrap().kind, b"comment");
	assert_eq!(segments.last().unwrap().name, b"database");
	assert_eq!(comment.signature, b"'@hall renamed-from council_member_id'");
}
