use std::collections::BTreeSet;
use std::sync::Arc;

use super::*;

fn source(file: usize, language: &str, path: &str) -> SourceFileRecord {
	SourceFileRecord {
		id: SourceId::at(file),
		uri: format!("code+moniker://./file:{path}"),
		source_root: 0,
		path: path.to_string(),
		rel_path: path.to_string(),
		anchor: path.to_string(),
		language: language.to_string(),
		text: String::new(),
	}
}

fn symbol(file: usize, def: usize, identity: &str, name: &str) -> SymbolRecord {
	let mut symbol = SymbolRecord::new(SymbolId::at(file, def), SourceId::at(file), name, "class");
	symbol.identity = Arc::from(identity);
	symbol.line_range = Some((3, 3));
	symbol
}

#[test]
fn indexes_facets_and_preserves_sparse_universe_complements() {
	let sources = vec![
		source(0, "java", "src/main/java/acme/infra/Good.java"),
		source(1, "java", "src/main/java/acme/domain/Bad.java"),
	];
	let symbols = RecordTable::from_shards(vec![
		Arc::from(vec![symbol(
			0,
			0,
			"code+moniker://./lang:java/srcset:main/package:acme/dir:infra/class:GoodRepository",
			"GoodRepository",
		)]),
		Arc::from(vec![symbol(
			1,
			0,
			"code+moniker://./lang:java/srcset:main/package:acme/dir:domain/class:BadRepository",
			"BadRepository",
		)]),
	]);
	let inventory = SymbolInventoryIndex::build(ResourceGeneration::new(7), &sources, &symbols);
	assert_eq!(inventory.all_symbols().len(), 2);
	assert_eq!(
		inventory
			.facets()
			.symbols_by_segment("dir", "infra")
			.expect("infra posting")
			.len(),
		1
	);
	assert_eq!(
		inventory
			.facets()
			.symbols_by_srcset("main")
			.expect("srcset posting")
			.len(),
		2
	);
	let infra = inventory
		.facets()
		.symbols_by_segment("dir", "infra")
		.expect("infra posting");
	let outside = inventory.all_symbols().difference(infra);
	assert_eq!(outside.len(), 1);
}

#[test]
fn natural_name_posting_groups_callable_overloads_without_scanning_names() {
	let sources = vec![source(0, "rs", "src/lib.rs")];
	let symbols = RecordTable::from_shards(vec![Arc::from(vec![
		symbol(0, 0, "code+moniker://./fn:get()", "get()"),
		symbol(0, 1, "code+moniker://./fn:get(u32)", "get(u32)"),
		symbol(0, 2, "code+moniker://./fn:getaway()", "getaway()"),
	])]);
	let inventory = SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &symbols);
	let matches = inventory
		.facets()
		.symbols_by_natural_name("get")
		.expect("natural name posting");
	assert_eq!(matches.len(), 2);
	assert!(inventory.facets().symbols_by_natural_name("geta").is_none());
}

#[test]
fn refresh_keeps_identity_ordinal_and_retires_removed_symbols() {
	let sources = vec![source(0, "java", "src/main/java/acme/Order.java")];
	let identity = "code+moniker://./lang:java/srcset:main/package:acme/class:OrderRepository";
	let before = RecordTable::from_shards(vec![Arc::from(vec![symbol(
		0,
		0,
		identity,
		"OrderRepository",
	)])]);
	let inventory = SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &before);
	let ordinal = inventory
		.facets()
		.symbols_by_identity(identity)
		.and_then(|symbols| symbols.iter().next())
		.expect("initial ordinal");
	let after = RecordTable::from_shards(vec![Arc::from(vec![symbol(
		0,
		1,
		identity,
		"OrderRepository",
	)])]);
	let refreshed = inventory.refresh(
		ResourceGeneration::new(2),
		&sources,
		&after,
		&BTreeSet::from([0]),
	);
	assert!(
		refreshed
			.facets()
			.symbols_by_identity(identity)
			.is_some_and(|symbols| symbols.contains(ordinal))
	);
	assert_eq!(refreshed.catalog().id(ordinal), Some(&SymbolId::at(0, 1)));
	assert_eq!(refreshed.all_symbols().len(), 1);

	let empty = RecordTable::from_shards(vec![Arc::from(Vec::<SymbolRecord>::new())]);
	let removed = refreshed.refresh(
		ResourceGeneration::new(3),
		&sources,
		&empty,
		&BTreeSet::from([0]),
	);
	assert!(removed.facets().symbols_by_identity(identity).is_none());
	assert!(removed.all_symbols().is_empty());
}

#[test]
fn duplicate_identities_keep_each_physical_symbol_addressable() {
	let sources = vec![
		source(0, "java", "module-a/src/test/java/acme/Duplicate.java"),
		source(1, "java", "module-b/src/test/java/acme/Duplicate.java"),
	];
	let identity = "code+moniker://./lang:java/srcset:test/package:acme/class:Duplicate";
	let symbols = RecordTable::from_shards(vec![
		Arc::from(vec![symbol(0, 0, identity, "Duplicate")]),
		Arc::from(vec![symbol(1, 0, identity, "Duplicate")]),
	]);

	let inventory = SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &symbols);
	let matches = inventory
		.facets()
		.symbols_by_identity(identity)
		.expect("identity posting");

	assert_eq!(matches.len(), 2);
	assert_eq!(inventory.catalog().ids(matches).len(), 2);
	assert!(inventory.record_by_id(&SymbolId::at(0, 0)).is_some());
	assert!(inventory.record_by_id(&SymbolId::at(1, 0)).is_some());

	let refreshed_symbols = RecordTable::from_shards(vec![
		Arc::from(vec![symbol(0, 1, identity, "Duplicate")]),
		Arc::from(vec![symbol(1, 0, identity, "Duplicate")]),
	]);
	let refreshed = inventory.refresh(
		ResourceGeneration::new(2),
		&sources,
		&refreshed_symbols,
		&BTreeSet::from([0]),
	);
	let matches = refreshed
		.facets()
		.symbols_by_identity(identity)
		.expect("refreshed identity posting");
	assert_eq!(matches.len(), 2);
	assert!(refreshed.record_by_id(&SymbolId::at(0, 1)).is_some());
	assert!(refreshed.record_by_id(&SymbolId::at(1, 0)).is_some());
}

#[test]
fn refresh_churn_does_not_grow_storage_with_retired_ordinal_holes() {
	let sources = vec![source(0, "java", "src/main/java/acme/Order.java")];
	let initial = RecordTable::from_shards(vec![Arc::from(vec![symbol(
		0,
		0,
		"code+moniker://./lang:java/srcset:main/package:acme/class:Order0",
		"Order0",
	)])]);
	let mut inventory = SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &initial);
	for generation in 2..=34 {
		let name = format!("Order{generation}");
		let identity = format!("code+moniker://./lang:java/srcset:main/package:acme/class:{name}");
		let symbols = RecordTable::from_shards(vec![Arc::from(vec![symbol(
			0, generation, &identity, &name,
		)])]);
		inventory = inventory.refresh(
			ResourceGeneration::new(generation as u64),
			&sources,
			&symbols,
			&BTreeSet::from([0]),
		);
	}
	assert_eq!(inventory.all_symbols().len(), 1);
	assert_eq!(inventory.catalog().len(), 1);
	assert!(
		inventory.catalog().storage_len() <= 2,
		"retired ordinals must not leave high-water storage: {} slots for one active symbol",
		inventory.catalog().storage_len()
	);
	assert!(
		inventory.records.len() <= 2,
		"record storage must stay proportional to active symbols: {} slots",
		inventory.records.len()
	);
}

#[test]
fn compact_identity_lookup_tracks_build_and_refresh() {
	let sources = vec![source(0, "rs", "src/lib.rs")];
	let before_identity = "code+moniker://./lang:rs/dir:src/class:Before";
	let before = RecordTable::from_shards(vec![Arc::from(vec![symbol(
		0,
		0,
		before_identity,
		"before",
	)])]);
	let inventory = SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &before);
	let before_compact = crate::code::compact_identity(before_identity, "code+moniker://")
		.expect("compact identity");
	assert_eq!(
		inventory.symbol_ids_by_compact_identity(&before_compact),
		vec![SymbolId::at(0, 0)]
	);

	let after_identity = "code+moniker://./lang:rs/dir:src/class:After";
	let after =
		RecordTable::from_shards(vec![Arc::from(vec![symbol(0, 0, after_identity, "after")])]);
	let refreshed = inventory.refresh(
		ResourceGeneration::new(2),
		&sources,
		&after,
		&BTreeSet::from([0]),
	);
	assert!(
		refreshed
			.symbol_ids_by_compact_identity(&before_compact)
			.is_empty()
	);
	let after_compact =
		crate::code::compact_identity(after_identity, "code+moniker://").expect("compact identity");
	assert_eq!(
		refreshed.symbol_ids_by_compact_identity(&after_compact),
		vec![SymbolId::at(0, 0)]
	);
}

#[test]
fn compact_identity_lookup_preserves_and_refreshes_ambiguity() {
	let sources = vec![source(0, "rs", "src/lib.rs")];
	let primary_identity = "code+moniker://./lang:rs/dir:src/class:Thing";
	let alternate_identity = "alternate://project/lang:rs/dir:src/class:Thing";
	let ambiguous = RecordTable::from_shards(vec![Arc::from(vec![
		symbol(0, 0, primary_identity, "primary"),
		symbol(0, 1, alternate_identity, "alternate"),
	])]);
	let inventory = SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &ambiguous);
	let compact = crate::code::compact_identity(primary_identity, "code+moniker://")
		.expect("compact identity");
	assert_eq!(
		inventory.symbol_ids_by_compact_identity(&compact),
		vec![SymbolId::at(0, 0), SymbolId::at(0, 1)]
	);

	let unambiguous = RecordTable::from_shards(vec![Arc::from(vec![symbol(
		0,
		0,
		primary_identity,
		"primary",
	)])]);
	let refreshed = inventory.refresh(
		ResourceGeneration::new(2),
		&sources,
		&unambiguous,
		&BTreeSet::from([0]),
	);
	assert_eq!(
		refreshed.symbol_ids_by_compact_identity(&compact),
		vec![SymbolId::at(0, 0)]
	);
}

#[test]
fn owner_descendant_posting_follows_parent_chain_across_refresh() {
	let sources = vec![source(0, "rs", "src/lib.rs")];
	let mut owner = symbol(
		0,
		0,
		"code+moniker://./lang:rs/dir:src/class:Owner",
		"Owner",
	);
	let mut method = symbol(
		0,
		1,
		"code+moniker://./lang:rs/dir:src/class:Owner/method:run",
		"run",
	);
	method.kind = "method".to_string();
	method.parent = Some(owner.id);
	let mut local = symbol(
		0,
		2,
		"code+moniker://./lang:rs/dir:src/class:Owner/method:run/local:item",
		"item",
	);
	local.kind = "local".to_string();
	local.parent = Some(method.id);
	let before = RecordTable::from_shards(vec![Arc::from(vec![
		owner.clone(),
		method.clone(),
		local.clone(),
	])]);
	let inventory = SymbolInventoryIndex::build(ResourceGeneration::new(1), &sources, &before);
	let owned = inventory.owner_and_descendants(&owner.id);
	assert_eq!(owned.len(), 3);
	assert!(
		inventory
			.catalog()
			.ordinal(&local.id)
			.is_some_and(|ordinal| owned.contains(ordinal))
	);

	owner.name = "Owner2".to_string();
	let after = RecordTable::from_shards(vec![Arc::from(vec![owner.clone()])]);
	let refreshed = inventory.refresh(
		ResourceGeneration::new(2),
		&sources,
		&after,
		&BTreeSet::from([0]),
	);
	assert_eq!(refreshed.owner_and_descendants(&owner.id).len(), 1);
	assert!(refreshed.facets().symbols_by_owner(&owner.id).is_none());
}
