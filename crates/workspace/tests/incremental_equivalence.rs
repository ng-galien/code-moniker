//! Incremental-vs-full equivalence oracle.
//!
//! After any sequence of source edits, an incremental refresh
//! (`refresh_paths` + `refresh_linkage`) must produce the same *observable*
//! index and linkage as a from-scratch build of the current on-disk state.
//! Snapshots are compared through a normal form keyed by identity URIs, so
//! generation counters and record ordering are free to differ.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use code_moniker_workspace::code::{CodeIndexPort, LocalCodeIndex, LocalCodeIndexOptions};
use code_moniker_workspace::linkage::{LinkageGraphDelta, LinkageRefreshImpact, LocalLinkage};
use code_moniker_workspace::snapshot::{
	CodeIndex, LinkageSnapshot, ReferenceId, SourceCatalog, SymbolId, WorkspaceRequest,
};
use code_moniker_workspace::source::{
	LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions, SourceCatalogPort,
};
use proptest::prelude::*;

#[derive(Debug, Eq, PartialEq)]
struct NormalForm {
	symbols: BTreeMap<String, Vec<String>>,
	references: BTreeMap<String, usize>,
	resolved: BTreeSet<String>,
	external: BTreeSet<String>,
	manifest_blocked: BTreeSet<String>,
	unresolved: BTreeSet<String>,
}

fn normal_form(index: &CodeIndex, linkage: &LinkageSnapshot) -> NormalForm {
	let identity_by_symbol: BTreeMap<&SymbolId, &str> = index
		.symbols
		.iter()
		.map(|symbol| (&symbol.id, symbol.identity.as_ref()))
		.collect();
	let symbol_identity =
		|id: &SymbolId| identity_by_symbol.get(id).copied().unwrap_or("<unknown>");

	let mut symbols: BTreeMap<String, Vec<String>> = BTreeMap::new();
	for symbol in index.symbols.iter() {
		let parent = symbol
			.parent
			.as_ref()
			.map(symbol_identity)
			.unwrap_or_default();
		symbols
			.entry(symbol.identity.to_string())
			.or_default()
			.push(format!(
				"name={} kind={} vis={} sig={} nav={} lines={:?} parent={parent}",
				symbol.name,
				symbol.kind,
				symbol.visibility,
				symbol.signature,
				symbol.navigable,
				symbol.line_range,
			));
	}
	symbols.values_mut().for_each(|entries| entries.sort());

	let mut reference_keys: BTreeMap<ReferenceId, String> = BTreeMap::new();
	let mut references: BTreeMap<String, usize> = BTreeMap::new();
	for reference in index.references.iter() {
		let key = format!(
			"from={} target={} kind={} call={:?} lines={:?}",
			symbol_identity(&reference.source_symbol),
			reference.target_identity,
			reference.kind,
			reference.call_name,
			reference.line_range,
		);
		reference_keys.insert(reference.id, key.clone());
		*references.entry(key).or_default() += 1;
	}
	let reference_key = |id: ReferenceId| {
		reference_keys
			.get(&id)
			.cloned()
			.unwrap_or_else(|| format!("<missing reference {id}>"))
	};

	NormalForm {
		symbols,
		references,
		resolved: linkage
			.resolved
			.iter()
			.map(|edge| {
				format!(
					"{} -> {}",
					reference_key(edge.reference),
					symbol_identity(&edge.target)
				)
			})
			.collect(),
		external: linkage
			.external
			.iter()
			.map(|external| reference_key(external.reference))
			.collect(),
		manifest_blocked: linkage
			.manifest_blocked
			.iter()
			.map(|blocked| reference_key(blocked.reference))
			.collect(),
		unresolved: linkage
			.unresolved
			.iter()
			.map(|unresolved| reference_key(unresolved.reference))
			.collect(),
	}
}

struct IncrementalSession {
	root: PathBuf,
	source_catalog: LocalSourceCatalog,
	code_index: LocalCodeIndex,
	linkage: LocalLinkage,
	catalog: SourceCatalog,
	index: CodeIndex,
	snapshot: LinkageSnapshot,
}

impl IncrementalSession {
	fn open(root: &Path) -> Self {
		let cache = LocalResourceCache::default();
		let mut source_catalog = LocalSourceCatalog::new(
			LocalSourceCatalogOptions::new(vec![root.to_path_buf()], None),
			cache.clone(),
		);
		let catalog = source_catalog
			.load_catalog(&WorkspaceRequest::new("equivalence-incremental"))
			.expect("catalog");
		let mut code_index = LocalCodeIndex::new(LocalCodeIndexOptions::new(None), cache.clone());
		let index = code_index.build_index(&catalog).expect("index");
		let mut linkage = LocalLinkage::new(cache);
		let snapshot = linkage
			.resolve_linkage_with_timings(&index)
			.expect("initial linkage")
			.snapshot;
		Self {
			root: root.to_path_buf(),
			source_catalog,
			code_index,
			linkage,
			catalog,
			index,
			snapshot,
		}
	}

	fn edit(&mut self, rel_path: &str, content: &str) {
		let path = self.root.join(rel_path);
		fs::write(&path, content).expect("write edit");
		let refreshed = self
			.code_index
			.refresh_paths(&self.index, std::slice::from_ref(&path))
			.expect("refresh paths");
		let impact = LinkageRefreshImpact::with_graph_delta(
			refreshed.changed_sources.clone(),
			vec![path],
			LinkageGraphDelta::from_code_index(refreshed.graph_diff.clone()),
		);
		self.snapshot = self
			.linkage
			.refresh_linkage_with_timings(&self.snapshot, &refreshed.index, impact)
			.expect("refresh linkage")
			.snapshot;
		self.index = refreshed.index;
		let _ = &self.catalog;
	}

	fn create(&mut self, rel_path: &str, content: &str) {
		let path = self.root.join(rel_path);
		fs::write(&path, content).expect("write new file");
		let extended = self
			.source_catalog
			.extend_catalog(&self.catalog, std::slice::from_ref(&path))
			.expect("extend catalog")
			.expect("path should extend the catalog");
		let refreshed = self
			.code_index
			.refresh_catalog_paths(&self.index, &extended, std::slice::from_ref(&path))
			.expect("refresh catalog paths");
		let impact = LinkageRefreshImpact::with_graph_delta(
			refreshed.changed_sources.clone(),
			vec![path],
			LinkageGraphDelta::from_code_index(refreshed.graph_diff.clone()),
		);
		self.snapshot = self
			.linkage
			.refresh_linkage_with_timings(&self.snapshot, &refreshed.index, impact)
			.expect("refresh linkage")
			.snapshot;
		self.index = refreshed.index;
		self.catalog = extended;
	}

	fn normal_form(&self) -> NormalForm {
		normal_form(&self.index, &self.snapshot)
	}
}

fn full_build_normal_form(root: &Path) -> NormalForm {
	let cache = LocalResourceCache::default();
	let mut source_catalog = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![root.to_path_buf()], None),
		cache.clone(),
	);
	let catalog = source_catalog
		.load_catalog(&WorkspaceRequest::new("equivalence-full"))
		.expect("catalog");
	let mut code_index = LocalCodeIndex::new(LocalCodeIndexOptions::new(None), cache.clone());
	let index = code_index.build_index(&catalog).expect("index");
	let mut linkage = LocalLinkage::new(cache);
	let snapshot = linkage
		.resolve_linkage_with_timings(&index)
		.expect("linkage")
		.snapshot;
	normal_form(&index, &snapshot)
}

const LIB_RS: &str = "pub mod alpha;\npub mod beta;\n";
const ALPHA_RS: &str = "pub fn shared() {}\npub fn helper() { shared(); }\n";
const BETA_RS: &str = "use crate::alpha::shared;\npub fn caller() { shared(); }\n";

fn seed_workspace() -> tempfile::TempDir {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	fs::write(src.join("lib.rs"), LIB_RS).expect("lib");
	fs::write(src.join("alpha.rs"), ALPHA_RS).expect("alpha");
	fs::write(src.join("beta.rs"), BETA_RS).expect("beta");
	temp
}

fn assert_equivalent_after(edits: &[(&str, &str)]) {
	let temp = seed_workspace();
	let mut session = IncrementalSession::open(temp.path());
	for (rel_path, content) in edits {
		session.edit(rel_path, content);
		let incremental = session.normal_form();
		let full = full_build_normal_form(temp.path());
		assert_eq!(
			incremental, full,
			"incremental refresh diverged from full rebuild after editing {rel_path}"
		);
	}
}

#[test]
fn editing_a_function_body_matches_full_rebuild() {
	assert_equivalent_after(&[(
		"src/beta.rs",
		"use crate::alpha::shared;\npub fn caller() { let _x = 1; shared(); }\n",
	)]);
}

#[test]
fn adding_a_definition_matches_full_rebuild() {
	assert_equivalent_after(&[(
		"src/alpha.rs",
		"pub fn shared() {}\npub fn helper() { shared(); }\npub fn added() {}\n",
	)]);
}

#[test]
fn removing_a_definition_matches_full_rebuild() {
	assert_equivalent_after(&[("src/alpha.rs", "pub fn shared() {}\n")]);
}

#[test]
fn renaming_a_cross_file_target_matches_full_rebuild() {
	assert_equivalent_after(&[(
		"src/alpha.rs",
		"pub fn renamed() {}\npub fn helper() { renamed(); }\n",
	)]);
}

#[test]
fn adding_then_removing_a_cross_file_call_matches_full_rebuild() {
	assert_equivalent_after(&[
		(
			"src/beta.rs",
			"use crate::alpha::{helper, shared};\npub fn caller() { shared(); helper(); }\n",
		),
		(
			"src/beta.rs",
			"use crate::alpha::shared;\npub fn caller() { shared(); }\n",
		),
	]);
}

#[test]
fn repeated_edits_of_the_same_file_match_full_rebuild() {
	assert_equivalent_after(&[
		("src/alpha.rs", "pub fn shared() {}\n"),
		(
			"src/alpha.rs",
			"pub fn shared() {}\npub fn helper() { shared(); }\n",
		),
		(
			"src/alpha.rs",
			"pub fn renamed() {}\npub fn helper() { renamed(); }\n",
		),
	]);
}

#[test]
fn creating_a_file_matches_full_rebuild() {
	let temp = seed_workspace();
	let mut session = IncrementalSession::open(temp.path());
	session.create(
		"src/gamma.rs",
		"use crate::alpha::shared;
pub fn gamma_caller() { shared(); }
",
	);
	assert_eq!(
		session.normal_form(),
		full_build_normal_form(temp.path()),
		"created file should index and link like a full rebuild"
	);
	session.edit(
		"src/gamma.rs",
		"use crate::alpha::shared;
pub fn gamma_caller() { shared(); shared(); }
",
	);
	assert_eq!(
		session.normal_form(),
		full_build_normal_form(temp.path()),
		"editing a created file should stay equivalent"
	);
}

#[test]
fn creating_a_file_that_targets_pending_references_matches_full_rebuild() {
	let temp = seed_workspace();
	let mut session = IncrementalSession::open(temp.path());
	session.edit(
		"src/beta.rs",
		"use crate::gamma::fresh;
pub fn caller() { fresh(); }
",
	);
	session.create(
		"src/gamma.rs",
		"pub fn fresh() {}
",
	);
	session.edit(
		"src/lib.rs",
		"pub mod alpha;
pub mod beta;
pub mod gamma;
",
	);
	assert_eq!(
		session.normal_form(),
		full_build_normal_form(temp.path()),
		"a created file should satisfy previously unresolved references"
	);
}

const ALPHA_VARIANTS: &[&str] = &[
	ALPHA_RS,
	"pub fn shared() {}\n",
	"pub fn renamed() {}\npub fn helper() { renamed(); }\n",
	"pub fn shared() {}\npub fn helper() { shared(); }\npub fn added() { helper(); }\n",
];

const BETA_VARIANTS: &[&str] = &[
	BETA_RS,
	"use crate::alpha::shared;\npub fn caller() { let _x = 1; shared(); }\n",
	"pub fn caller() {}\n",
	"use crate::alpha::{helper, shared};\npub fn caller() { shared(); helper(); }\n",
];

proptest! {
	#![proptest_config(ProptestConfig { cases: 8, ..ProptestConfig::default() })]
	#[test]
	fn random_edit_sequences_match_full_rebuild(
		steps in proptest::collection::vec((0usize..2, 0usize..4), 1..5)
	) {
		let temp = seed_workspace();
		let mut session = IncrementalSession::open(temp.path());
		for (file_choice, variant) in steps {
			let (rel_path, content) = match file_choice {
				0 => ("src/alpha.rs", ALPHA_VARIANTS[variant]),
				_ => ("src/beta.rs", BETA_VARIANTS[variant]),
			};
			session.edit(rel_path, content);
			prop_assert_eq!(
				session.normal_form(),
				full_build_normal_form(temp.path()),
				"incremental refresh diverged from full rebuild after editing {}",
				rel_path
			);
		}
	}
}
