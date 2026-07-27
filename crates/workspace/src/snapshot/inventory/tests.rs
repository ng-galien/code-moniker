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
		.catalog()
		.ordinal_by_identity(identity)
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
	assert_eq!(
		refreshed.catalog().ordinal_by_identity(identity),
		Some(ordinal)
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
	assert!(removed.catalog().ordinal_by_identity(identity).is_none());
	assert!(removed.all_symbols().is_empty());
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
