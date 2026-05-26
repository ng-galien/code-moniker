use std::fs;
use std::path::PathBuf;

use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::Lang;

use crate::check::workspace::{WorkspaceCheckRunner, WorkspaceCheckRunnerOptions};
use crate::workspace::SessionStoreBridge;
use crate::workspace::changes::analyzer::ChangeAnalyzer;
use crate::workspace::changes::{ChangeOverlayPort, LocalChangeOverlay};
use crate::workspace::code::{
	CodeIndexPort, CodeIndexSymbolProvider, LocalCodeIndex, LocalCodeIndexOptions,
	NormalizedSource, NormalizedSymbol, SymbolProvider,
};
use crate::workspace::facade::{LocalWorkspaceFacade, LocalWorkspaceOptions};
use crate::workspace::git;
use crate::workspace::linkage::{LinkagePort, LocalLinkage};
use crate::workspace::snapshot::{
	ChangeStatus, CodeIndex, LinkageGraph, SymbolLocation, WorkspaceRequest,
	WorkspaceSnapshotRefresh, WorkspaceTransition, WorkspaceView,
};
use crate::workspace::source::{
	LocalIdentityResolver, LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions,
	SourceCatalogPort,
};
use crate::workspace::store::IndexStore;

#[test]
fn local_resources_extract_sources_symbols_and_linkage() {
	let fixture = LocalFixture::new();
	let cache = LocalResourceCache::default();
	let mut catalog_port = fixture.source_catalog(cache.clone());
	let mut index_port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone());
	let mut linkage_port = LocalLinkage::new(cache);

	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("local"))
		.expect("catalog loads");
	let index = index_port.build_index(&catalog).expect("index builds");
	let linkage = linkage_port
		.resolve_linkage(&index)
		.expect("linkage resolves");

	assert_eq!(catalog.sources.len(), 1);
	assert!(
		index
			.symbols
			.iter()
			.any(|symbol| symbol.name.contains("callee"))
	);
	assert!(!index.references.is_empty());
	assert!(linkage.resolved_refs > 0);
}

#[test]
fn local_identity_resolver_controls_source_and_symbol_uris() {
	let fixture = LocalFixture::new();
	let identity = LocalIdentityResolver::new("custom+moniker://");
	let cache = LocalResourceCache::default();
	let mut catalog_port = fixture.source_catalog_with_identity(cache.clone(), identity);
	let mut index_port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache);

	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("local"))
		.expect("catalog loads");
	let index = index_port.build_index(&catalog).expect("index builds");

	assert_eq!(index.identity_scheme, "custom+moniker://");
	assert!(
		index
			.sources
			.iter()
			.all(|source| source.uri.starts_with("custom+moniker://"))
	);
	assert!(
		index
			.symbols
			.iter()
			.all(|symbol| symbol.identity.starts_with("custom+moniker://"))
	);
}

#[test]
fn source_catalog_resolves_canonical_uri_for_known_paths() {
	let fixture = LocalFixture::new();
	let cache = LocalResourceCache::default();
	let mut catalog_port = fixture.source_catalog(cache.clone());

	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("local"))
		.expect("catalog loads");
	let material = cache
		.source_material(catalog.generation)
		.expect("source material is cached");
	let source_file = fixture.dir.path().join("src/lib.rs");
	let from_absolute = material
		.source_uri_for_path(&source_file)
		.expect("absolute path resolves");
	let from_relative = material
		.source_uri_for_path(&PathBuf::from("src/lib.rs"))
		.expect("relative path resolves");

	assert_eq!(from_absolute, from_relative);
	assert!(from_absolute.starts_with(crate::DEFAULT_SCHEME));
}

#[test]
fn symbol_provider_loads_rule_derived_source_outside_eager_index() {
	let fixture = LazyJavaFixture::new();
	let cache = LocalResourceCache::default();
	let mut catalog_port = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![fixture.root.clone()], Some("lazy-java".into()))
			.with_files(vec![fixture.main_source.clone()]),
		cache.clone(),
	);
	let mut index_port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone());

	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("lazy-java"))
		.expect("catalog loads eager source");
	let index = index_port.build_index(&catalog).expect("index builds");
	let material = cache
		.index_material(index.generation)
		.expect("index material cached");
	let provider = CodeIndexSymbolProvider::new(&material);
	let lazy_source = provider
		.source_for_path(&fixture.test_source)
		.expect("rule-derived test source resolves");
	let lazy_symbols = provider
		.symbols_for_path(&fixture.test_source)
		.expect("rule-derived test symbols load");

	assert_eq!(index.sources.len(), 1);
	assert_eq!(lazy_source.language, Lang::Java);
	assert!(lazy_source.uri.starts_with(crate::DEFAULT_SCHEME));
	assert!(
		lazy_source
			.rel_path
			.ends_with("src/test/java/acme/FooTest.java")
	);
	assert!(
		lazy_symbols
			.iter()
			.any(|symbol| symbol.identity.contains("FooTest"))
	);
}

#[test]
fn local_code_index_reports_source_read_failure() {
	let fixture = LocalFixture::new();
	let cache = LocalResourceCache::default();
	let mut catalog_port = fixture.source_catalog(cache.clone());
	let mut index_port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache);

	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("local"))
		.expect("catalog loads");
	std::fs::remove_file(fixture.dir.path().join("src/lib.rs")).expect("remove source");
	let failure = index_port
		.build_index(&catalog)
		.expect_err("missing source must fail index build");

	assert!(failure.message.contains("cannot read") || failure.message.contains("cannot extract"));
}

#[test]
fn local_session_builds_complete_snapshot_from_real_resources() {
	let fixture = LocalFixture::new();
	let cache = LocalResourceCache::default();
	let mut session = fixture.workspace_session(cache);

	let transition = session.refresh(WorkspaceRequest::new("local"));
	let snapshot = session.snapshot().expect("snapshot is published");

	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	assert_eq!(snapshot.catalog.sources.len(), 1);
	assert!(
		snapshot
			.index
			.symbols
			.iter()
			.any(|symbol| symbol.name.contains("caller"))
	);
	assert!(snapshot.linkage.resolved_refs > 0);
}

#[test]
fn workspace_facade_builds_snapshot_from_injected_local_services() {
	let fixture = LocalFixture::new();
	let mut workspace = LocalWorkspaceFacade::local(LocalWorkspaceOptions::new(
		vec![fixture.dir.path().to_path_buf()],
		Some("demo".into()),
	));

	let transition = workspace.refresh(WorkspaceRequest::new("local"));
	let view = workspace.view().expect("workspace view is published");

	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	assert_eq!(view.sources().all().len(), 1);
	assert!(
		view.symbols()
			.all()
			.iter()
			.any(|symbol| symbol.name.contains("caller"))
	);
}

#[test]
fn local_session_view_exposes_contract_read_models() {
	let fixture = LocalFixture::new();
	let mut session = fixture.workspace_session(LocalResourceCache::default());

	session.refresh(WorkspaceRequest::new("local"));
	let snapshot = session.snapshot().expect("snapshot is published");
	let view = WorkspaceView::new(snapshot);
	let sources = view.sources().all();
	let symbols = view.symbols().all();
	let caller = symbols
		.iter()
		.find(|symbol| symbol.name.contains("caller"))
		.expect("caller symbol");
	let callee = symbols
		.iter()
		.find(|symbol| symbol.name.contains("callee"))
		.expect("callee symbol");
	let references = view
		.references()
		.for_symbol(&callee.id)
		.expect("callee references");

	assert_eq!(sources.len(), 1);
	assert!(view.symbols().detail(&caller.id).is_some());
	assert!(
		view.search()
			.search_symbols("caller", 10)
			.iter()
			.any(|hit| hit.symbol == caller.id)
	);
	assert_eq!(references.incoming.summary.refs, 1);
	assert_eq!(
		view.changes().summaries().len(),
		snapshot.changes.changes.len()
	);
	assert_eq!(
		view.linkage().unresolved_report().unresolved_refs,
		snapshot.linkage.unresolved_refs
	);
}

#[test]
fn session_store_bridge_exposes_index_store_surface() {
	let fixture = LocalFixture::new();
	let bridge = SessionStoreBridge::load(fixture.session_options()).expect("session bridge loads");
	let bridge_callee = bridge_def_named(&bridge, "callee");
	let bridge_caller = bridge_def_named(&bridge, "caller");
	let bridge_check = bridge
		.check_summary(&fixture.rules, None, crate::DEFAULT_SCHEME)
		.expect("bridge check summary");

	assert!(
		bridge
			.root()
			.contains(fixture.dir.path().to_string_lossy().as_ref())
	);
	assert_eq!(bridge.stats().files, 1);
	assert!(bridge.stats().defs >= 2);
	assert!(bridge.stats().refs > 0);
	assert!(bridge.linkage_stats().resolved_refs > 0);
	assert_eq!(bridge.file_count(), 1);
	assert!(bridge.file_summary(0).rel_path.ends_with("src/lib.rs"));
	assert!(bridge.all_navigable_defs().len() >= 2);
	assert!(!bridge.root_defs(0).is_empty());
	assert!(bridge.is_navigable_symbol(&bridge_callee));
	assert!(
		bridge
			.symbol_summary(&bridge_callee)
			.name
			.contains("callee")
	);
	assert!(
		bridge
			.symbol_summary(&bridge_caller)
			.name
			.contains("caller")
	);
	assert!(!bridge.source_snippet(&bridge_caller, 1).is_empty());
	assert_eq!(
		bridge
			.symbol_references(&bridge_callee)
			.incoming
			.summary
			.refs,
		1
	);
	assert!(
		bridge
			.search_symbols_filtered("caller", 10, &[], &[], &[])
			.iter()
			.any(|hit| hit.loc == bridge_caller)
	);
	assert!(bridge.change_overview().change_count <= bridge.change_rows().len());
	assert!(bridge.usage_focus(bridge_callee).references.summary.refs > 0);
	assert_eq!(bridge.unresolved_linkage_report(10, 3).unresolved_refs, 0);
	assert!(bridge_check.total_violations > 0);
}

#[test]
fn local_check_runner_counts_rule_severity() {
	let fixture = LocalFixture::new();
	let cache = LocalResourceCache::default();
	let mut catalog_port = fixture.source_catalog(cache.clone());
	let mut index_port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone());
	let mut linkage_port = LocalLinkage::new(cache.clone());
	let mut check_runner = WorkspaceCheckRunner::new(fixture.check_options(), cache);

	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("local"))
		.expect("catalog loads");
	let index = index_port.build_index(&catalog).expect("index builds");
	let linkage = linkage_port
		.resolve_linkage(&index)
		.expect("linkage resolves");
	let diagnostics = check_runner
		.run_check(&index, &linkage)
		.expect("diagnostics collect");

	assert_eq!(diagnostics.errors, 0);
	assert!(diagnostics.warnings > 0);
	assert_eq!(diagnostics.diagnostics.len(), diagnostics.warnings);
}

#[test]
fn local_check_runner_maps_symbols_with_custom_scheme() {
	let fixture = LocalFixture::new();
	let cache = LocalResourceCache::default();
	let mut catalog_port = fixture.source_catalog(cache.clone());
	let mut index_port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone());
	let mut linkage_port = LocalLinkage::new(cache.clone());
	let mut check_runner = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(fixture.rules.clone(), None, "custom+moniker://"),
		cache,
	);

	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("local"))
		.expect("catalog loads");
	let index = index_port.build_index(&catalog).expect("index builds");
	let linkage = linkage_port
		.resolve_linkage(&index)
		.expect("linkage resolves");
	let diagnostics = check_runner
		.run_check(&index, &linkage)
		.expect("diagnostics collect");

	assert!(diagnostics.warnings > 0);
	assert!(
		diagnostics
			.diagnostics
			.iter()
			.all(|diagnostic| diagnostic.symbol.is_some())
	);
}

#[test]
fn change_analyzer_preserves_metadata_without_current_symbol() {
	let entry = git::ChangeEntry {
		loc: None,
		status: git::ChangeStatus::Removed,
		lang: Lang::Rs,
		file_path: PathBuf::from("src/lib.rs"),
		kind: "function".to_string(),
		name: "removed".to_string(),
		moniker: sample_moniker("removed"),
		hunk_count: 2,
		line_range: Some((3, 5)),
	};
	let provider = EmptySymbolProvider;

	let records = ChangeAnalyzer::new(&provider).analyze(&[entry]);
	let record = records.first().expect("change record");

	assert_eq!(record.source, None);
	assert_eq!(record.symbol, None);
	assert_eq!(record.language, Lang::Rs.tag());
	assert_eq!(record.file_path, "src/lib.rs");
	assert!(record.identity.starts_with(crate::DEFAULT_SCHEME));
	assert_eq!(record.hunk_count, 2);
}

#[test]
fn local_change_overlay_reports_modified_and_removed_symbols_from_git() {
	let fixture = GitChangeFixture::new();
	let cache = LocalResourceCache::default();
	let mut catalog_port = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![fixture.root.clone()], Some("git-change".into())),
		cache.clone(),
	);
	let mut index_port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone());
	let mut linkage_port = LocalLinkage::new(cache.clone());
	let mut change_port = LocalChangeOverlay::new(cache);

	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("git-change"))
		.expect("catalog loads");
	let index = index_port.build_index(&catalog).expect("index builds");
	let linkage = linkage_port
		.resolve_linkage(&index)
		.expect("linkage resolves");
	let changes = change_port
		.build_change_overlay(&catalog, &index, &linkage)
		.expect("change overlay builds");

	assert!(
		changes
			.changes
			.iter()
			.any(|change| change.status == ChangeStatus::Modified && change.name == "kept()")
	);
	assert!(
		changes
			.changes
			.iter()
			.any(|change| change.status == ChangeStatus::Removed && change.name == "removed()")
	);
}

#[test]
fn bridge_load_does_not_compile_check_rules() {
	let fixture = LocalFixture::new();
	let invalid_rules = fixture.invalid_rules();

	let bridge = SessionStoreBridge::load(fixture.session_options());

	assert!(bridge.is_ok());
	let check = bridge.expect("bridge loads without rules").check_summary(
		&invalid_rules,
		None,
		crate::DEFAULT_SCHEME,
	);
	assert!(check.is_err());
}

#[test]
fn java_multiprojet_source_folder_has_complete_new_linkage() {
	let root =
		PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace/java/multiprojet");
	let cache = LocalResourceCache::default();
	let mut catalog_port = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![root], Some("multiprojet".into())),
		cache.clone(),
	);
	let mut index_port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone());
	let mut linkage_port = LocalLinkage::new(cache);

	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("multiprojet"))
		.expect("catalog loads");
	let index = index_port.build_index(&catalog).expect("index builds");
	let linkage = linkage_port
		.resolve_linkage(&index)
		.expect("linkage resolves");

	assert_eq!(index.sources.len(), 14);
	assert_eq!(index.symbols.len(), 121);
	assert_eq!(index.references.len(), 251);
	assert_eq!(linkage.unresolved_refs, 0);
	assert_eq!(linkage.manifest_blocked_refs, 0);
}

#[test]
fn java_linkage_reports_manifest_blocked_cross_project_reference() {
	let fixture = JavaLinkageFixture::new();
	let common = fixture.root.join("common-lib");
	let app = fixture.root.join("app");
	write_file(
		&common,
		PathBuf::from("pom.xml").as_path(),
		&pom("com.acme", "common-lib", &[]),
	);
	write_file(
		&common,
		PathBuf::from("src/main/java/com/acme/common/Shared.java").as_path(),
		"package com.acme.common;\npublic class Shared {}\n",
	);
	write_file(
		&app,
		PathBuf::from("pom.xml").as_path(),
		&pom("com.acme", "app", &[]),
	);
	write_file(
		&app,
		PathBuf::from("src/main/java/com/acme/app/App.java").as_path(),
		"package com.acme.app;\nimport com.acme.common.Shared;\npublic class App { private final Shared shared = new Shared(); }\n",
	);

	let (_index, linkage) = workspace_index_and_linkage(vec![common, app], Some("blocked"));

	assert!(linkage.manifest_blocked_refs > 0);
	assert_eq!(linkage.unresolved_refs, 0);
}

#[test]
fn java_linkage_reports_ambiguous_reference_candidates() {
	let fixture = JavaLinkageFixture::new();
	write_file(
		&fixture.root,
		PathBuf::from("src/main/java/acme/one/Dupe.java").as_path(),
		"package acme;\npublic class Dupe {}\n",
	);
	write_file(
		&fixture.root,
		PathBuf::from("src/main/java/acme/two/Dupe.java").as_path(),
		"package acme;\npublic class Dupe {}\n",
	);
	write_file(
		&fixture.root,
		PathBuf::from("src/main/java/acme/App.java").as_path(),
		"package acme;\npublic class App { private Dupe dupe; }\n",
	);

	let (_index, linkage) =
		workspace_index_and_linkage(vec![fixture.root.clone()], Some("ambiguous"));

	assert!(linkage.ambiguous_refs > 0);
	assert_eq!(linkage.unresolved_refs, 0);
}

#[test]
fn java_linkage_resolves_projectless_cross_project_reference_by_manifest() {
	let fixture = JavaLinkageFixture::new();
	let common = fixture.root.join("common-lib");
	let app = fixture.root.join("app");
	write_file(
		&common,
		PathBuf::from("pom.xml").as_path(),
		&pom("com.acme", "common-lib", &[]),
	);
	write_file(
		&common,
		PathBuf::from("src/main/java/com/acme/common/Shared.java").as_path(),
		"package com.acme.common;\npublic class Shared {}\n",
	);
	write_file(
		&app,
		PathBuf::from("pom.xml").as_path(),
		&pom("com.acme", "app", &[("com.acme", "common-lib")]),
	);
	write_file(
		&app,
		PathBuf::from("src/main/java/com/acme/app/App.java").as_path(),
		"package com.acme.app;\nimport com.acme.common.Shared;\npublic class App { private final Shared shared = new Shared(); }\n",
	);

	let (_index, linkage) = workspace_index_and_linkage(vec![common, app], None);

	assert!(linkage.resolved_refs > 0);
	assert_eq!(linkage.manifest_blocked_refs, 0);
	assert_eq!(linkage.unresolved_refs, 0);
}

struct EmptySymbolProvider;

impl SymbolProvider for EmptySymbolProvider {
	fn source_at(&self, _file_idx: usize) -> Option<NormalizedSource> {
		None
	}

	fn source_for_path(&self, _path: &std::path::Path) -> Option<NormalizedSource> {
		None
	}

	fn symbol_at(&self, _loc: SymbolLocation) -> Option<NormalizedSymbol> {
		None
	}

	fn symbol_for_moniker(&self, _moniker: &Moniker) -> Option<NormalizedSymbol> {
		None
	}

	fn symbols_for_path(&self, _path: &std::path::Path) -> Option<Vec<NormalizedSymbol>> {
		None
	}

	fn identity_for_moniker(&self, moniker: &Moniker) -> String {
		LocalIdentityResolver::default().moniker_uri(moniker)
	}
}

fn sample_moniker(name: &str) -> Moniker {
	MonikerBuilder::new()
		.project(b"demo")
		.segment(b"fn", name.as_bytes())
		.build()
}

struct LocalFixture {
	dir: tempfile::TempDir,
	rules: std::path::PathBuf,
}

struct LazyJavaFixture {
	_dir: tempfile::TempDir,
	root: PathBuf,
	main_source: PathBuf,
	test_source: PathBuf,
}

impl LazyJavaFixture {
	fn new() -> Self {
		let dir = tempfile::tempdir().expect("tempdir");
		let root = dir.path().to_path_buf();
		let main_source = PathBuf::from("src/main/java/acme/Foo.java");
		let test_source = root.join("src/test/java/acme/FooTest.java");
		write_file(
			&root,
			main_source.as_path(),
			"package acme;\npublic class Foo {}\n",
		);
		write_file(
			&root,
			PathBuf::from("src/test/java/acme/FooTest.java").as_path(),
			"package acme;\npublic class FooTest {}\n",
		);
		Self {
			_dir: dir,
			root,
			main_source,
			test_source,
		}
	}
}

struct JavaLinkageFixture {
	_dir: tempfile::TempDir,
	root: PathBuf,
}

impl JavaLinkageFixture {
	fn new() -> Self {
		let dir = tempfile::tempdir().expect("tempdir");
		Self {
			root: dir.path().to_path_buf(),
			_dir: dir,
		}
	}
}

struct GitChangeFixture {
	_dir: tempfile::TempDir,
	root: PathBuf,
}

impl GitChangeFixture {
	fn new() -> Self {
		let dir = tempfile::tempdir().expect("tempdir");
		let root = dir.path().to_path_buf();
		write_file(
			&root,
			PathBuf::from("src/lib.rs").as_path(),
			"pub fn kept() {}\npub fn removed() {}\n",
		);
		run_git(&root, &["init"]);
		run_git(&root, &["add", "."]);
		run_git(
			&root,
			&[
				"-c",
				"user.name=Code Moniker",
				"-c",
				"user.email=code-moniker@example.invalid",
				"commit",
				"-m",
				"initial",
			],
		);
		write_file(
			&root,
			PathBuf::from("src/lib.rs").as_path(),
			"pub fn kept() { let value = 1; let _ = value; }\npub fn added() {}\n",
		);
		Self { _dir: dir, root }
	}
}

fn write_file(root: &std::path::Path, rel: &std::path::Path, body: &str) {
	let path = root.join(rel);
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent).expect("fixture parent");
	}
	fs::write(path, body).expect("fixture source");
}

fn run_git(root: &std::path::Path, args: &[&str]) {
	let output = std::process::Command::new("git")
		.arg("-C")
		.arg(root)
		.args(args)
		.output()
		.unwrap_or_else(|err| panic!("cannot run git {args:?}: {err}"));
	assert!(
		output.status.success(),
		"git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);
}

fn workspace_index_and_linkage(
	paths: Vec<PathBuf>,
	project: Option<&str>,
) -> (CodeIndex, LinkageGraph) {
	let cache = LocalResourceCache::default();
	let mut catalog_port = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(paths, project.map(ToOwned::to_owned)),
		cache.clone(),
	);
	let mut index_port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone());
	let mut linkage_port = LocalLinkage::new(cache);
	let catalog = catalog_port
		.load_catalog(&WorkspaceRequest::new("linkage"))
		.expect("catalog loads");
	let index = index_port.build_index(&catalog).expect("index builds");
	let linkage = linkage_port
		.resolve_linkage(&index)
		.expect("linkage resolves");
	(index, linkage)
}

fn pom(group: &str, artifact: &str, deps: &[(&str, &str)]) -> String {
	let dependencies = deps
		.iter()
		.map(|(dep_group, dep_artifact)| {
			format!(
				r#"<dependency><groupId>{dep_group}</groupId><artifactId>{dep_artifact}</artifactId><version>1.0.0</version></dependency>"#
			)
		})
		.collect::<String>();
	format!(
		r#"<project><modelVersion>4.0.0</modelVersion><groupId>{group}</groupId><artifactId>{artifact}</artifactId><version>1.0.0</version><dependencies>{dependencies}</dependencies></project>"#
	)
}

impl LocalFixture {
	fn new() -> Self {
		let dir = tempfile::tempdir().expect("tempdir");
		let src = dir.path().join("src");
		fs::create_dir(&src).expect("src dir");
		fs::write(
			src.join("lib.rs"),
			r#"
fn callee() {}

fn caller() {
	callee();
}
"#,
		)
		.expect("source");
		let rules = dir.path().join(".code-moniker.toml");
		fs::write(
			&rules,
			r#"
[[rust.fn.where]]
id = "workspace-session-warn-functions"
severity = "warn"
expr = "name =~ ^$"
message = "function is visible to workspace session diagnostics"
"#,
		)
		.expect("rules");
		Self { dir, rules }
	}

	fn source_catalog(&self, cache: LocalResourceCache) -> LocalSourceCatalog {
		self.source_catalog_with_identity(cache, LocalIdentityResolver::default())
	}

	fn source_catalog_with_identity(
		&self,
		cache: LocalResourceCache,
		identity: LocalIdentityResolver,
	) -> LocalSourceCatalog {
		LocalSourceCatalog::new(
			LocalSourceCatalogOptions::new(
				vec![self.dir.path().to_path_buf()],
				Some("demo".into()),
			)
			.with_identity(identity),
			cache,
		)
	}

	fn check_options(&self) -> WorkspaceCheckRunnerOptions {
		WorkspaceCheckRunnerOptions::new(self.rules.clone(), None, crate::DEFAULT_SCHEME)
	}

	fn invalid_rules(&self) -> std::path::PathBuf {
		let invalid_rules = self.dir.path().join("invalid.code-moniker.toml");
		fs::write(&invalid_rules, "not valid toml =").expect("invalid rules");
		invalid_rules
	}

	fn session_options(&self) -> crate::workspace::index::SessionOptions {
		crate::workspace::index::SessionOptions {
			paths: vec![self.dir.path().to_path_buf()],
			project: Some("demo".into()),
			cache_dir: None,
		}
	}

	fn workspace_session(
		&self,
		cache: LocalResourceCache,
	) -> WorkspaceSnapshotRefresh<
		LocalSourceCatalog,
		LocalCodeIndex,
		LocalLinkage,
		LocalChangeOverlay,
	> {
		WorkspaceSnapshotRefresh::new(
			self.source_catalog(cache.clone()),
			LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone()),
			LocalLinkage::new(cache.clone()),
			LocalChangeOverlay::new(cache.clone()),
		)
	}
}

fn bridge_def_named(store: &SessionStoreBridge, name: &str) -> crate::workspace::DefLocation {
	store
		.all_navigable_defs()
		.into_iter()
		.find(|loc| store.symbol_summary(loc).name.contains(name))
		.expect("bridge symbol")
}
