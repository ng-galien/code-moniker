use code_moniker_core::core::moniker::MonikerBuilder;
use code_moniker_core::lang::{LangExtractor, sql};

#[test]
fn indexes_preserve_order_expressions_include_predicates_and_table_links() {
	let source = r#"CREATE TABLE app.child ("A" int, b int REFERENCES parent(id));
CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS child_idx ON app.child
USING btree ("A" DESC NULLS LAST, lower(b::text), (b + 1)) INCLUDE (b) WHERE b > 0;"#;
	let parsed = sql::Lang::parse("index.sql", source);
	assert!(
		!parsed.primary().root_node().has_error(),
		"{}",
		parsed.primary().root_node().to_sexp()
	);
	let anchor = MonikerBuilder::new().project(b"test").build();
	let graph = sql::extract(
		"index.sql",
		source,
		&anchor,
		false,
		&sql::Presets::default(),
	);
	let signatures = |kind: &[u8]| {
		graph
			.defs()
			.filter(|d| d.kind.as_ref() == kind)
			.map(|d| String::from_utf8_lossy(&d.signature).into_owned())
			.collect::<Vec<_>>()
	};
	assert_eq!(signatures(b"index"), ["btree"]);
	let keys = signatures(b"index_key");
	assert_eq!(keys.len(), 3);
	assert!(keys[0].contains("A"));
	assert_eq!(
		&keys[1..],
		["expression:lower(b::text)", "expression:(b + 1)"]
	);
	assert_eq!(signatures(b"index_include"), ["column:b"]);
	assert_eq!(signatures(b"index_predicate"), ["WHERE b > 0"]);
	assert_eq!(signatures(b"constraint_column"), ["column:b"]);
	let index = graph.defs().find(|d| d.kind.as_ref() == b"index").unwrap();
	assert!(graph.refs().any(|r| {
		graph.def_at(r.source).moniker == index.moniker
			&& r.kind.as_ref() == b"member_of"
			&& graph
				.defs()
				.any(|d| d.moniker == r.target && d.kind.as_ref() == b"table")
	}));
	for def in graph.defs().filter(|d| d.kind.as_ref() == b"index_key") {
		assert_eq!(def.moniker.parent().as_ref(), Some(&index.moniker));
		let (start, end) = def.position.unwrap();
		assert!(!source[start as usize..end as usize].is_empty());
	}
}
