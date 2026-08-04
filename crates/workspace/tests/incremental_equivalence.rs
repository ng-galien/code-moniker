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
use code_moniker_workspace::linkage::{
	LinkageGraphDelta, LinkageMemoryMetrics, LinkageRefreshImpact, LocalLinkage,
};
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
	candidates: BTreeSet<String>,
	dynamic: BTreeSet<String>,
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
		candidates: linkage
			.candidates
			.iter()
			.map(|candidate| {
				let mut targets = candidate
					.targets
					.iter()
					.map(symbol_identity)
					.collect::<Vec<_>>();
				targets.sort();
				format!(
					"{} -> {:?} reason={} scope={}",
					reference_key(candidate.reference),
					targets,
					candidate.reason.as_str(),
					candidate.scope.as_str()
				)
			})
			.collect(),
		dynamic: linkage
			.dynamic
			.iter()
			.map(|dynamic| {
				let mut candidates = dynamic
					.candidates
					.iter()
					.map(symbol_identity)
					.collect::<Vec<_>>();
				candidates.sort();
				format!(
					"{} -> {:?} reason={}",
					reference_key(dynamic.reference),
					candidates,
					dynamic.reason.as_str()
				)
			})
			.collect(),
		external: external_normal_form(linkage, &reference_keys),
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

fn external_normal_form(
	linkage: &LinkageSnapshot,
	reference_keys: &BTreeMap<ReferenceId, String>,
) -> BTreeSet<String> {
	linkage
		.external
		.iter()
		.map(|external| {
			let reference = reference_keys
				.get(&external.reference)
				.cloned()
				.unwrap_or_else(|| format!("<missing reference {}>", external.reference));
			format!(
				"{reference} -> {} origin={}",
				external.target_identity,
				external.origin.label(),
			)
		})
		.collect()
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

	fn edit(&mut self, rel_path: &str, content: &str) -> usize {
		self.edit_with_metrics(rel_path, content).0
	}

	fn edit_with_metrics(
		&mut self,
		rel_path: &str,
		content: &str,
	) -> (usize, LinkageMemoryMetrics) {
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
		let refreshed_linkage = self
			.linkage
			.refresh_linkage_with_timings(&self.snapshot, &refreshed.index, impact)
			.expect("refresh linkage");
		let changed_refs = refreshed_linkage.timings.changed_refs;
		let memory = refreshed_linkage.memory;
		self.snapshot = refreshed_linkage.snapshot;
		self.index = refreshed.index;
		let _ = &self.catalog;
		(changed_refs, memory)
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

	fn remove(&mut self, rel_path: &str) -> usize {
		let path = self.root.join(rel_path);
		fs::remove_file(&path).expect("remove file");
		let extended = self
			.source_catalog
			.extend_catalog(&self.catalog, std::slice::from_ref(&path))
			.expect("extend catalog")
			.expect("removal should produce a catalog delta");
		let refreshed = self
			.code_index
			.refresh_catalog_paths(&self.index, &extended, std::slice::from_ref(&path))
			.expect("refresh catalog paths");
		let impact = LinkageRefreshImpact::with_graph_delta(
			refreshed.changed_sources.clone(),
			vec![path],
			LinkageGraphDelta::from_code_index(refreshed.graph_diff.clone()),
		);
		let refreshed_linkage = self
			.linkage
			.refresh_linkage_with_timings(&self.snapshot, &refreshed.index, impact)
			.expect("refresh linkage");
		let changed_refs = refreshed_linkage.timings.changed_refs;
		self.snapshot = refreshed_linkage.snapshot;
		self.index = refreshed.index;
		self.catalog = extended;
		changed_refs
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

fn seed_python_workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
	let temp = tempfile::tempdir().expect("tempdir");
	for (path, content) in files {
		fs::write(temp.path().join(path), content).expect("python fixture");
	}
	temp
}

fn seed_csharp_workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
	let temp = tempfile::tempdir().expect("tempdir");
	for (path, content) in files {
		fs::write(temp.path().join(path), content).expect("C# fixture");
	}
	temp
}

fn seed_c_workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
	let temp = tempfile::tempdir().expect("tempdir");
	for (path, content) in files {
		fs::write(temp.path().join(path), content).expect("C fixture");
	}
	temp
}

fn seed_sql_workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
	let temp = tempfile::tempdir().expect("tempdir");
	for (path, content) in files {
		fs::write(temp.path().join(path), content).expect("SQL fixture");
	}
	temp
}

#[test]
fn cross_language_name_partition_changes_match_full_rebuild() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(
		temp.path().join("caller.rs"),
		"struct Caller;\nimpl Caller { fn call(&self) { self.foreignOnly(); } }\n",
	)
	.expect("Rust fixture");
	fs::write(
		temp.path().join("foreign.ts"),
		"export class Foreign { foreignOnly(): void {} }\n",
	)
	.expect("TypeScript fixture");
	let mut session = IncrementalSession::open(temp.path());
	session.edit(
		"foreign.ts",
		"export class Foreign { renamedOnly(): void {} }\n",
	);

	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
	assert!(
		session.snapshot.resolved.iter().all(|edge| {
			let reference = session
				.index
				.references
				.iter()
				.find(|reference| reference.id == edge.reference)
				.expect("resolved reference");
			reference.call_name.as_deref() != Some("foreignOnly")
		}),
		"Rust calls must not resolve through a TypeScript name partition"
	);
}

#[test]
fn cross_file_rust_receiver_method_changes_match_full_rebuild() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	fs::write(
		src.join("lib.rs"),
		"mod receiver;\nuse receiver::Value;\nfn duplicate(value: &Value) -> Value { value.clone() }\n",
	)
	.expect("Rust caller fixture");
	fs::write(src.join("receiver.rs"), "pub struct Value;\n").expect("Rust receiver fixture");
	let mut session = IncrementalSession::open(temp.path());
	let clone_reference = session
		.index
		.references
		.iter()
		.find(|reference| {
			reference.kind == "method_call" && reference.call_name.as_deref() == Some("clone")
		})
		.expect("clone reference")
		.id;
	assert!(
		session.snapshot.external.iter().any(|external| {
			external.reference == clone_reference && external.target_identity.contains("sdk:rs")
		}),
		"an imported receiver without a workspace method should retain the SDK fallback"
	);
	session.edit(
		"src/receiver.rs",
		"pub struct Value;\nimpl Value { pub fn clone(&self) -> Self { Self } }\n",
	);

	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
}

#[test]
fn sql_default_arity_changes_match_full_rebuild() {
	let temp = seed_sql_workspace(&[
		(
			"definitions.sql",
			"CREATE FUNCTION public.finish(value int) RETURNS void LANGUAGE sql AS $$ SELECT value $$;\n",
		),
		("usage.sql", "SELECT public.finish();\n"),
	]);
	let mut session = IncrementalSession::open(temp.path());
	session.edit(
		"definitions.sql",
		"CREATE FUNCTION public.finish(value int DEFAULT 1) RETURNS void LANGUAGE sql AS $$ SELECT value $$;\n",
	);

	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
	assert_eq!(session.snapshot.unresolved_refs, 0);
}

#[test]
fn sql_typed_call_changes_match_full_rebuild() {
	let temp = seed_sql_workspace(&[
		(
			"definitions.sql",
			"CREATE FUNCTION public.choose(value int) RETURNS void LANGUAGE sql AS $$ SELECT value $$;\nCREATE FUNCTION public.choose(value text) RETURNS void LANGUAGE sql AS $$ SELECT value $$;\n",
		),
		("usage.sql", "SELECT public.choose(1::int);\n"),
	]);
	let mut session = IncrementalSession::open(temp.path());
	session.edit("usage.sql", "SELECT public.choose('one'::text);\n");

	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
}

#[test]
fn csharp_attribute_short_name_changes_match_full_rebuild() {
	let temp = seed_csharp_workspace(&[
		("Program.cs", "[Marker] public class Program {}\n"),
		("Marker.cs", "public class OtherAttribute {}\n"),
	]);
	let mut session = IncrementalSession::open(temp.path());
	session.edit("Marker.cs", "public class MarkerAttribute {}\n");

	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
}

#[test]
fn csharp_typed_receiver_changes_match_full_rebuild() {
	let temp = seed_csharp_workspace(&[
		(
			"Program.cs",
			"public class Program { public void Run() { Worker value = new Worker(); value.Format(); } }\n",
		),
		(
			"Worker.cs",
			"public class Worker { public void Format() {} }\n",
		),
		(
			"Rival.cs",
			"public class Rival { public void Format() {} }\n",
		),
	]);
	let mut session = IncrementalSession::open(temp.path());
	session.edit(
		"Program.cs",
		"public class Program { public void Run() { Rival value = new Rival(); value.Format(); } }\n",
	);

	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
}

#[test]
fn c_call_arity_changes_match_full_rebuild() {
	let temp = seed_c_workspace(&[
		(
			"math.c",
			"int add(int left, int right) { return left + right; }\n",
		),
		("main.c", "int run(void) { return add(1); }\n"),
	]);
	let mut session = IncrementalSession::open(temp.path());
	session.edit("main.c", "int run(void) { return add(1, 2); }\n");

	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
}

#[test]
fn c_include_visibility_changes_match_full_rebuild() {
	let temp = seed_c_workspace(&[
		("types.h", "typedef struct Item { int value; } Item;\n"),
		(
			"fragment.c",
			"int read_item(Item *item) { return item->value; }\n",
		),
		("main.c", "#include \"fragment.c\"\n"),
	]);
	let mut session = IncrementalSession::open(temp.path());
	assert!(
		session.snapshot.unresolved_refs > 0,
		"the fragment must not see a type from a header that is not included"
	);
	session.edit("main.c", "#include \"types.h\"\n#include \"fragment.c\"\n");

	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
	assert_eq!(session.snapshot.unresolved_refs, 0);
}

#[test]
fn changing_python_union_return_types_matches_full_rebuild() {
	let temp = seed_python_workspace(&[
		(
			"models.py",
			"class Alpha:\n    def render(self): pass\n\nclass Beta:\n    def render(self): pass\n\nclass Gamma:\n    def render(self): pass\n",
		),
		(
			"factory.py",
			"from models import Alpha, Beta, Gamma\n\ndef make() -> Alpha | Beta:\n    return Alpha()\n",
		),
		(
			"consumer.py",
			"from factory import make\n\ndef consume():\n    return make().render()\n",
		),
	]);
	let mut session = IncrementalSession::open(temp.path());
	session.edit(
		"factory.py",
		"from models import Alpha, Beta, Gamma\n\ndef make() -> Alpha | Gamma:\n    return Alpha()\n",
	);
	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
}

#[test]
fn changing_python_structural_method_set_matches_full_rebuild() {
	let temp = seed_python_workspace(&[
		(
			"protocols.py",
			"class Alpha:\n    def first(self): pass\n    def second(self): pass\n\nclass Beta:\n    def first(self): pass\n    def second(self): pass\n\nclass Gamma:\n    def first(self): pass\n",
		),
		(
			"consumer.py",
			"def consume(value):\n    value.first()\n    value.second()\n",
		),
	]);
	let mut session = IncrementalSession::open(temp.path());
	session.edit(
		"protocols.py",
		"class Alpha:\n    def first(self): pass\n    def second(self): pass\n\nclass Beta:\n    def first(self): pass\n    def second(self): pass\n\nclass Gamma:\n    def first(self): pass\n    def second(self): pass\n",
	);
	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
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
fn adding_an_unreferenced_definition_refreshes_the_candidate_catalog() {
	let temp = seed_workspace();
	let mut session = IncrementalSession::open(temp.path());
	let (_, memory) = session.edit_with_metrics(
		"src/alpha.rs",
		"pub fn shared() {}\npub fn helper() { shared(); }\npub fn added() {}\n",
	);

	assert_eq!(memory.symbol_catalog_entries, session.index.symbols.len());
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

#[test]
fn removing_a_file_matches_full_rebuild() {
	let temp = seed_workspace();
	let mut session = IncrementalSession::open(temp.path());
	session.edit("src/lib.rs", "pub mod alpha;\n");
	session.remove("src/beta.rs");
	assert_eq!(
		session.normal_form(),
		full_build_normal_form(temp.path()),
		"removing a file should match a full rebuild"
	);
}

#[test]
fn removing_a_referenced_target_file_matches_full_rebuild() {
	let temp = seed_workspace();
	let mut session = IncrementalSession::open(temp.path());
	session.edit("src/lib.rs", "pub mod beta;\n");
	session.remove("src/alpha.rs");
	assert_eq!(
		session.normal_form(),
		full_build_normal_form(temp.path()),
		"references into the removed file should become unresolved like a full rebuild"
	);
}

#[test]
fn removing_then_recreating_a_file_matches_full_rebuild() {
	let temp = seed_workspace();
	let mut session = IncrementalSession::open(temp.path());
	session.remove("src/beta.rs");
	session.create(
		"src/beta.rs",
		"use crate::alpha::shared;\npub fn caller_reborn() { shared(); }\n",
	);
	assert_eq!(
		session.normal_form(),
		full_build_normal_form(temp.path()),
		"a recreated file should re-index in its original slot"
	);
}

#[test]
fn changing_python_all_relinks_unchanged_wildcard_consumers() {
	let temp = tempfile::tempdir().expect("tempdir");
	let package = temp.path().join("sample");
	fs::create_dir_all(&package).expect("package");
	fs::write(package.join("__init__.py"), "from sample.impl import *\n").expect("facade");
	fs::write(
		package.join("impl.py"),
		"__all__ = ['Client']\nclass Client:\n    pass\n",
	)
	.expect("implementation");
	fs::write(
		temp.path().join("consumer.py"),
		"from sample import Client\ndef build():\n    return Client()\n",
	)
	.expect("consumer");
	let mut session = IncrementalSession::open(temp.path());

	session.edit("sample/impl.py", "__all__ = []\nclass Client:\n    pass\n");

	assert_eq!(
		session.normal_form(),
		full_build_normal_form(temp.path()),
		"changing __all__ must invalidate unchanged wildcard consumers"
	);
}

#[test]
fn removing_an_ordinary_python_call_does_not_relink_the_workspace() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(
		temp.path().join("provider.py"),
		"def stable():\n    return 1\n",
	)
	.expect("provider");
	fs::write(
		temp.path().join("consumer.py"),
		"from provider import stable\ndef run():\n    return stable()\n",
	)
	.expect("consumer");
	fs::write(
		temp.path().join("unrelated.py"),
		"def helper():\n    return 2\ndef run():\n    return helper()\n",
	)
	.expect("unrelated");
	let mut session = IncrementalSession::open(temp.path());

	let changed_refs = session.edit(
		"consumer.py",
		"from provider import stable\ndef run():\n    return 0\n",
	);

	assert_eq!(changed_refs, 0, "a removed call is not a binding change");
	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
}

#[test]
fn removing_a_python_facade_relinks_unchanged_consumers() {
	let temp = tempfile::tempdir().expect("tempdir");
	let package = temp.path().join("sample");
	fs::create_dir_all(&package).expect("package");
	fs::write(
		package.join("__init__.py"),
		"from sample.facade import Client\n",
	)
	.expect("package init");
	fs::write(
		package.join("facade.py"),
		"from sample.impl import Client\n",
	)
	.expect("facade");
	fs::write(package.join("impl.py"), "class Client:\n    pass\n").expect("implementation");
	fs::write(
		temp.path().join("consumer.py"),
		"from sample import Client\ndef build():\n    return Client()\n",
	)
	.expect("consumer");
	fs::write(
		temp.path().join("unrelated.py"),
		"def helper():\n    return 1\ndef run():\n    return helper()\n",
	)
	.expect("unrelated");
	let mut session = IncrementalSession::open(temp.path());
	let total_references = session.index.references.len();

	let changed_refs = session.remove("sample/facade.py");

	assert!(
		changed_refs < total_references,
		"removing one facade must only relink its dependency closure"
	);
	assert_eq!(
		session.normal_form(),
		full_build_normal_form(temp.path()),
		"removing a facade must invalidate unchanged consumers"
	);
}

#[test]
fn removing_an_unreferenced_python_module_has_an_empty_dependency_closure() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(
		temp.path().join("orphan.py"),
		"def orphan():\n    return 1\n",
	)
	.expect("orphan");
	fs::write(
		temp.path().join("unrelated.py"),
		"def helper():\n    return 2\ndef run():\n    return helper()\n",
	)
	.expect("unrelated");
	let mut session = IncrementalSession::open(temp.path());

	let changed_refs = session.remove("orphan.py");

	assert_eq!(
		changed_refs, 0,
		"an unreferenced deleted module has no dependent references"
	);
	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));
}

#[test]
fn manifest_origin_refresh_matches_full_rebuild_across_sibling_projects() {
	let temp = tempfile::tempdir().expect("tempdir");
	for project in ["a", "b"] {
		fs::create_dir_all(temp.path().join(project).join("src")).expect("project src");
		fs::write(
			temp.path().join(project).join("src/store.ts"),
			"import { create } from \"zustand\";\nexport const useStore = create(() => ({ count: 0 }));\n",
		)
		.expect("store");
		fs::write(
			temp.path().join(project).join("src/app.ts"),
			format!(
				"import {{ useStore }} from \"./store\";\nexport function from{project}() {{ return useStore.getState().count; }}\n"
			),
		)
		.expect("app");
	}
	fs::write(
		temp.path().join("a/package.json"),
		"{\"name\":\"a\",\"dependencies\":{\"zustand\":\"^5.0.0\"}}\n",
	)
	.expect("manifest");
	let mut session = IncrementalSession::open(temp.path());
	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));

	session.edit("a/package.json", "{\"name\":\"a\",\"dependencies\":{}}\n");

	assert_eq!(
		session.normal_form(),
		full_build_normal_form(temp.path()),
		"manifest edits must reclassify semantic external origins exactly like a full rebuild",
	);
}

#[test]
fn rust_custom_lib_name_refresh_matches_full_rebuild() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::create_dir_all(temp.path().join("src")).expect("src");
	fs::write(
		temp.path().join("Cargo.toml"),
		"[package]\nname = \"demo-cli\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
	)
	.expect("manifest");
	fs::write(
		temp.path().join("src/lib.rs"),
		"pub fn run() -> &'static str { \"linked\" }\n",
	)
	.expect("library");
	fs::write(
		temp.path().join("src/main.rs"),
		"fn main() { let _value = demo_cli::run(); }\n",
	)
	.expect("binary");
	let mut session = IncrementalSession::open(temp.path());
	assert_eq!(session.normal_form(), full_build_normal_form(temp.path()));

	session.edit(
		"Cargo.toml",
		"[package]\nname = \"demo-cli\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\nname = \"demo_runtime\"\n",
	);
	let renamed = session.normal_form();
	assert_eq!(renamed, full_build_normal_form(temp.path()));
	assert!(
		renamed
			.resolved
			.iter()
			.all(|edge| !edge.contains("external_pkg:demo_cli/path:run")),
		"[lib].name must retire the package-derived crate root",
	);

	session.edit(
		"src/main.rs",
		"fn main() { let _value = demo_runtime::run(); }\n",
	);
	let renamed_call = session.normal_form();
	assert_eq!(
		renamed_call,
		full_build_normal_form(temp.path()),
		"custom Rust lib names must resolve identically after incremental manifest and source refresh",
	);
	assert!(
		renamed_call
			.resolved
			.iter()
			.any(|edge| edge.contains("external_pkg:demo_runtime/path:run")),
		"the renamed library crate root must resolve after incremental refresh",
	);
}

#[test]
fn typescript_sdk_profile_source_edits_match_full_rebuild() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(
		temp.path().join("tsconfig.json"),
		r#"{"compilerOptions":{"target":"ES2022","lib":["ES2022","DOM"]}}"#,
	)
	.expect("tsconfig");
	fs::write(
		temp.path().join("app.ts"),
		"export function render(button: HTMLButtonElement) { button.replaceChildren(); }\n",
	)
	.expect("TypeScript fixture");
	let mut session = IncrementalSession::open(temp.path());

	session.edit(
		"app.ts",
		"export function render(button: HTMLButtonElement) { button.classList.add('ready'); document.body.replaceChildren(button); }\n",
	);

	let incremental = session.normal_form();
	assert_eq!(
		incremental,
		full_build_normal_form(temp.path()),
		"TypeScript SDK references must stay equivalent under incremental source refresh",
	);
	assert!(
		incremental
			.external
			.iter()
			.any(|reference| reference.contains("sdk:ts")),
		"the DOM profile must retain SDK provenance after refresh",
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
