use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{
	CodeIndex, SymbolId, WorkspaceRequest, WorkspaceTransition,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct OracleSymbol {
	id: SymbolId,
	identity: String,
	name: String,
	kind: String,
	lang: String,
	srcset: String,
	package: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct OracleGroupKey {
	lang: String,
	srcset: String,
	package: String,
	name: String,
}

fn write(root: &Path, path: &str, source: &str) {
	let target = root.join(path);
	fs::create_dir_all(target.parent().expect("fixture parent")).expect("fixture directory");
	fs::write(target, source).expect("fixture source");
}

fn fixture_index() -> (tempfile::TempDir, CodeIndex) {
	let dir = tempfile::tempdir().expect("workspace-rule fixture");
	write(
		dir.path(),
		"src/main/java/com/acme/infra/GoodRepository.java",
		"package com.acme.infra; public class GoodRepository {}\n",
	);
	write(
		dir.path(),
		"src/main/java/com/acme/domain/BadRepository.java",
		"package com.acme.domain; public class BadRepository {}\n",
	);
	write(
		dir.path(),
		"src/main/java/com/acme/sales/FirstOrder.java",
		"package com.acme.sales; class Order {}\n",
	);
	write(
		dir.path(),
		"src/main/java/com/acme/sales/SecondOrder.java",
		"package com.acme.sales; class Order {}\n",
	);
	write(
		dir.path(),
		"src/test/java/com/acme/sales/OrderTest.java",
		"package com.acme.sales; class Order {}\n",
	);

	let mut registry = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
		vec![dir.path().to_path_buf()],
		None,
	));
	let transition = registry
		.commands()
		.refresh(WorkspaceRequest::new("workspace-rule-oracle"));
	assert!(
		matches!(transition, WorkspaceTransition::Ready { .. }),
		"fixture indexing failed: {:?}",
		registry.queries().last_failure()
	);
	let index = registry
		.queries()
		.snapshot()
		.expect("workspace-rule fixture snapshot")
		.index
		.clone();
	(dir, index)
}

fn segment(identity: &str, kind: &str) -> String {
	identity
		.split('/')
		.filter_map(|part| part.split_once(':'))
		.filter(|(segment_kind, _)| *segment_kind == kind)
		.map(|(_, name)| name)
		.collect::<Vec<_>>()
		.join(".")
}

fn symbols(index: &CodeIndex) -> Vec<OracleSymbol> {
	index
		.symbols
		.iter()
		.map(|symbol| {
			let source = &index.sources[symbol.source.file()];
			OracleSymbol {
				id: symbol.id,
				identity: symbol.identity.to_string(),
				name: symbol.name.clone(),
				kind: symbol.kind.clone(),
				lang: source.language.clone(),
				srcset: segment(&symbol.identity, "srcset"),
				package: segment(&symbol.identity, "package"),
			}
		})
		.collect()
}

fn repository_placement_violations(symbols: &[OracleSymbol]) -> Vec<SymbolId> {
	symbols
		.iter()
		.filter(|symbol| {
			symbol.kind == "class"
				&& symbol.name.ends_with("Repository")
				&& !symbol
					.identity
					.split('/')
					.any(|part| part == "dir:infra" || part == "package:infra")
		})
		.map(|symbol| symbol.id)
		.collect()
}

fn duplicate_type_groups(symbols: &[OracleSymbol]) -> BTreeMap<OracleGroupKey, Vec<SymbolId>> {
	let mut groups = BTreeMap::<OracleGroupKey, Vec<SymbolId>>::new();
	for symbol in symbols.iter().filter(|symbol| symbol.kind == "class") {
		groups
			.entry(OracleGroupKey {
				lang: symbol.lang.clone(),
				srcset: symbol.srcset.clone(),
				package: symbol.package.clone(),
				name: symbol.name.clone(),
			})
			.or_default()
			.push(symbol.id);
	}
	groups.retain(|_, members| members.len() > 1);
	groups
}

#[test]
fn oracle_finds_cross_file_placement_and_group_violations() {
	let (_fixture, index) = fixture_index();
	let symbols = symbols(&index);

	let placement = repository_placement_violations(&symbols);
	assert_eq!(placement.len(), 1);
	let bad = symbols
		.iter()
		.find(|symbol| symbol.id == placement[0])
		.expect("bad repository");
	assert_eq!(bad.name, "BadRepository");

	let duplicates = duplicate_type_groups(&symbols);
	assert_eq!(duplicates.len(), 1, "{duplicates:#?}");
	let (key, members) = duplicates.first_key_value().expect("duplicate group");
	assert_eq!(key.lang, "java");
	assert_eq!(key.srcset, "main");
	assert_eq!(key.package, "com.acme.sales");
	assert_eq!(key.name, "Order");
	assert_eq!(members.len(), 2);
}

fn oracle_symbol(file: usize, def: usize, lang: &str, srcset: &str, package: &str) -> OracleSymbol {
	OracleSymbol {
		id: SymbolId::at(file, def),
		identity: format!(
			"code+moniker://fixture/lang:{lang}/srcset:{srcset}/package:{package}/class:Order"
		),
		name: "Order".to_string(),
		kind: "class".to_string(),
		lang: lang.to_string(),
		srcset: srcset.to_string(),
		package: package.to_string(),
	}
}

#[test]
fn oracle_group_key_isolates_language_srcset_and_package() {
	let symbols = vec![
		oracle_symbol(0, 0, "java", "main", "com.acme.sales"),
		oracle_symbol(1, 0, "java", "main", "com.acme.sales"),
		oracle_symbol(2, 0, "java", "test", "com.acme.sales"),
		oracle_symbol(3, 0, "java", "main", "com.acme.support"),
		oracle_symbol(4, 0, "python", "main", "com.acme.sales"),
	];

	let duplicates = duplicate_type_groups(&symbols);
	assert_eq!(duplicates.len(), 1, "{duplicates:#?}");
	let (key, members) = duplicates.first_key_value().expect("duplicate group");
	assert_eq!(
		key,
		&OracleGroupKey {
			lang: "java".to_string(),
			srcset: "main".to_string(),
			package: "com.acme.sales".to_string(),
			name: "Order".to_string(),
		}
	);
	assert_eq!(members, &vec![SymbolId::at(0, 0), SymbolId::at(1, 0)]);
}
