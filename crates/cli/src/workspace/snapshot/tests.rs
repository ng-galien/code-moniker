use std::fs;
use std::path::PathBuf;

use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::core::shape::Shape;
use code_moniker_core::lang::Lang;

use crate::check::workspace::{WorkspaceCheckRunner, WorkspaceCheckRunnerOptions};
use crate::workspace::SessionStoreBridge;
use crate::workspace::changes::LocalChangeOverlay;
use crate::workspace::changes::analyzer::ChangeAnalyzer;
use crate::workspace::code::{
	CodeIndexPort, LocalCodeIndex, LocalCodeIndexOptions, NormalizedSource, NormalizedSymbol,
	SymbolProvider,
};
use crate::workspace::git;
use crate::workspace::linkage::{LinkagePort, LocalLinkage};
use crate::workspace::snapshot::{
	SymbolLocation, WorkspaceRequest, WorkspaceSnapshotRefresh, WorkspaceTransition, WorkspaceView,
};
use crate::workspace::source::{
	LocalIdentityResolver, LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions,
	SourceCatalogPort,
};
use crate::workspace::store::{IndexStore, WorkspaceStore};

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
fn local_session_view_matches_legacy_core_contracts() {
	let fixture = LocalFixture::new();
	let legacy = WorkspaceStore::load(&fixture.session_options()).expect("legacy store loads");
	let mut session = fixture.workspace_session(LocalResourceCache::default());

	session.refresh(WorkspaceRequest::new("local"));
	let snapshot = session.snapshot().expect("snapshot is published");
	let view = WorkspaceView::new(snapshot);
	let symbols = view.symbols().all();
	let legacy_symbols = legacy.all_navigable_defs();
	let new_callee = symbols
		.iter()
		.find(|symbol| symbol.name.contains("callee"))
		.expect("new callee");
	let legacy_callee = legacy_symbols
		.iter()
		.find(|loc| legacy.symbol_summary(loc).name.contains("callee"))
		.expect("legacy callee");

	assert_eq!(view.sources().all().len(), legacy.file_count());
	assert_eq!(symbols.len(), legacy_symbols.len());
	assert_eq!(
		view.references()
			.for_symbol(&new_callee.id)
			.expect("new callee references")
			.incoming
			.summary
			.refs,
		legacy
			.symbol_references(legacy_callee)
			.incoming
			.summary
			.refs
	);
	assert_eq!(
		view.search().search_symbols("caller", 10).len(),
		legacy
			.search_symbols_filtered("caller", 10, &[], &[], &[])
			.len()
	);
	assert_eq!(view.changes().summaries().len(), legacy.change_rows().len());
}

#[test]
fn session_store_bridge_matches_legacy_index_store_surface() {
	let fixture = LocalFixture::new();
	let legacy = WorkspaceStore::load(&fixture.session_options()).expect("legacy store loads");
	let bridge = SessionStoreBridge::load(fixture.session_options()).expect("session bridge loads");
	let legacy_callee = legacy_def_named(&legacy, "callee");
	let bridge_callee = bridge_def_named(&bridge, "callee");
	let legacy_caller = legacy_def_named(&legacy, "caller");
	let bridge_caller = bridge_def_named(&bridge, "caller");
	let legacy_check = legacy
		.check_summary(&fixture.rules, None, crate::DEFAULT_SCHEME)
		.expect("legacy check summary");
	let bridge_check = bridge
		.check_summary(&fixture.rules, None, crate::DEFAULT_SCHEME)
		.expect("bridge check summary");

	assert_eq!(bridge.root(), legacy.root());
	assert_eq!(bridge.stats().files, legacy.stats().files);
	assert_eq!(bridge.stats().defs, legacy.stats().defs);
	assert_eq!(bridge.stats().refs, legacy.stats().refs);
	assert_eq!(
		bridge.linkage_stats().resolved_refs,
		legacy.linkage_stats().resolved_refs
	);
	assert_eq!(bridge.file_count(), legacy.file_count());
	assert_eq!(bridge.file_summary(0), legacy.file_summary(0));
	assert_eq!(
		bridge.all_navigable_defs().len(),
		legacy.all_navigable_defs().len()
	);
	assert_eq!(bridge.root_defs(0).len(), legacy.root_defs(0).len());
	assert_eq!(
		bridge.is_navigable_symbol(&bridge_callee),
		legacy.is_navigable_symbol(&legacy_callee)
	);
	assert_eq!(
		bridge.symbol_summary(&bridge_callee).name,
		legacy.symbol_summary(&legacy_callee).name
	);
	assert_eq!(
		bridge.symbol_detail(&bridge_caller).children.len(),
		legacy.symbol_detail(&legacy_caller).children.len()
	);
	assert_eq!(
		bridge
			.symbol_references(&bridge_callee)
			.incoming
			.summary
			.refs,
		legacy
			.symbol_references(&legacy_callee)
			.incoming
			.summary
			.refs
	);
	assert_eq!(
		bridge
			.symbol_references(&bridge_caller)
			.outgoing
			.summary
			.refs,
		legacy
			.symbol_references(&legacy_caller)
			.outgoing
			.summary
			.refs
	);
	assert_eq!(
		bridge.source_snippet(&bridge_caller, 1).len(),
		legacy.source_snippet(&legacy_caller, 1).len()
	);
	assert_eq!(
		bridge
			.search_symbols_filtered("caller", 10, &[], &[], &[])
			.len(),
		legacy
			.search_symbols_filtered("caller", 10, &[], &[], &[])
			.len()
	);
	assert_eq!(
		bridge
			.search_symbols_filtered("caller", 10, &[], &["function".to_string()], &[])
			.len(),
		legacy
			.search_symbols_filtered("caller", 10, &[], &["function".to_string()], &[])
			.len()
	);
	assert_eq!(
		bridge
			.search_symbols_filtered("caller", 10, &[], &[], &[Shape::Callable])
			.len(),
		legacy
			.search_symbols_filtered("caller", 10, &[], &[], &[Shape::Callable])
			.len()
	);
	assert_eq!(bridge.change_overview(), legacy.change_overview());
	assert_eq!(bridge.change_rows(), legacy.change_rows());
	assert_eq!(bridge.changed_defs(), legacy.changed_defs());
	assert_eq!(
		bridge.change_count_for_file(0),
		legacy.change_count_for_file(0)
	);
	assert_eq!(
		bridge.usage_focus(bridge_callee).references.summary.refs,
		legacy.usage_focus(legacy_callee).references.summary.refs
	);
	assert_eq!(
		bridge.unresolved_linkage_report(10, 3).unresolved_refs,
		legacy.unresolved_linkage_report(10, 3).unresolved_refs
	);
	assert_eq!(bridge_check.total_violations, legacy_check.total_violations);
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

struct EmptySymbolProvider;

impl SymbolProvider for EmptySymbolProvider {
	fn source_at(&self, _file_idx: usize) -> Option<NormalizedSource> {
		None
	}

	fn symbol_at(&self, _loc: SymbolLocation) -> Option<NormalizedSymbol> {
		None
	}

	fn symbol_for_moniker(&self, _moniker: &Moniker) -> Option<NormalizedSymbol> {
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

fn legacy_def_named(store: &WorkspaceStore, name: &str) -> crate::workspace::DefLocation {
	store
		.all_navigable_defs()
		.into_iter()
		.find(|loc| store.symbol_summary(loc).name.contains(name))
		.expect("legacy symbol")
}

fn bridge_def_named(store: &SessionStoreBridge, name: &str) -> crate::workspace::DefLocation {
	store
		.all_navigable_defs()
		.into_iter()
		.find(|loc| store.symbol_summary(loc).name.contains(name))
		.expect("bridge symbol")
}
