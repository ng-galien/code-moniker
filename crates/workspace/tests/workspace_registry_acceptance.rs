use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use code_moniker_workspace::code::{
	CodeIndexPort, CodeIndexRefresh, LocalCodeIndex, LocalCodeIndexOptions,
};
use code_moniker_workspace::linkage::{LinkageGraphDelta, LinkageRefreshImpact, LocalLinkage};
use code_moniker_workspace::live::{
	LiveWorkspaceWatcher, WorkspaceLiveEvent, WorkspaceLiveRefreshPlan,
};
use code_moniker_workspace::registry::{
	LocalWorkspaceOptions, LocalWorkspaceRegistry, WorkspaceCommandKind, WorkspaceCommandSpec,
	WorkspaceEventKind, WorkspaceScopeUri, WorkspaceSnapshotPublication,
};
use code_moniker_workspace::snapshot::{
	CodeIndex, LinkageSnapshot, WorkspaceRequest, WorkspaceResource, WorkspaceTransition,
};
use code_moniker_workspace::source::{
	LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions, SourceCatalogPort,
};

fn fixture_path(path: impl AsRef<Path>) -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures")
		.join(path)
}

#[test]
fn refresh_paths_plans_stale_unresolved_refs_when_provider_symbols_change() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	let consumer = src.join("consumer.rs");
	let provider = src.join("provider.rs");
	fs::write(&lib, "pub mod consumer;\npub mod provider;\n").expect("write lib");
	fs::write(
		&consumer,
		"use crate::provider::provided;\npub fn call_provider() { provided(); }\n",
	)
	.expect("write consumer");
	fs::write(&provider, "").expect("write provider");
	let mut workspace = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
		vec![temp.path().to_path_buf()],
		None,
	));

	assert!(matches!(
		workspace
			.commands()
			.refresh(WorkspaceRequest::new("initial-refresh")),
		WorkspaceTransition::Ready { .. }
	));
	let initial = workspace.queries().snapshot().expect("initial snapshot");
	assert!(
		!reference_resolves_to_symbol(initial, "provided", "provided"),
		"`provided` should start unresolved before provider exports it"
	);

	fs::write(&provider, "pub fn provided() {}\n").expect("rewrite provider");
	assert!(matches!(
		workspace.commands().refresh_paths(
			WorkspaceRequest::new("provider-live-refresh"),
			vec![provider]
		),
		WorkspaceTransition::Ready { .. }
	));
	let refreshed = workspace.queries().snapshot().expect("refreshed snapshot");

	assert!(
		reference_resolves_to_symbol(refreshed, "provided", "provided"),
		"`provided` reference from unchanged consumer should be resolved after provider refresh"
	);
}

#[test]
fn refresh_paths_plans_stale_manifest_policy_refs_when_manifest_changes() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let manifest = temp.path().join("package.json");
	let app = src.join("app.ts");
	fs::write(
		&manifest,
		r#"{"name":"manifest-live-refresh","version":"1.0.0","dependencies":{}}"#,
	)
	.expect("write manifest");
	fs::write(
		&app,
		"import { create } from 'zustand';\nexport const store = create(() => ({ count: 0 }));\n",
	)
	.expect("write app");
	let mut workspace = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
		vec![temp.path().to_path_buf()],
		None,
	));

	assert!(matches!(
		workspace
			.commands()
			.refresh(WorkspaceRequest::new("initial-refresh")),
		WorkspaceTransition::Ready { .. }
	));
	let initial = workspace.queries().snapshot().expect("initial snapshot");
	assert!(
		!reference_is_external(initial, "external_pkg:zustand/function:create"),
		"`zustand/create` should not be external before the manifest declares zustand"
	);

	fs::write(
		&manifest,
		r#"{"name":"manifest-live-refresh","version":"1.0.0","dependencies":{"zustand":"latest"}}"#,
	)
	.expect("rewrite manifest");
	assert!(matches!(
		workspace.commands().refresh_paths(
			WorkspaceRequest::new("manifest-live-refresh"),
			vec![manifest]
		),
		WorkspaceTransition::Ready { .. }
	));
	let refreshed = workspace.queries().snapshot().expect("refreshed snapshot");

	assert!(
		reference_is_external(refreshed, "external_pkg:zustand/function:create"),
		"`zustand/create` should become external after manifest refresh"
	);
	assert!(
		!reference_is_unresolved(refreshed, "external_pkg:zustand/function:create"),
		"`zustand/create` stale unresolved projection should be invalidated"
	);
}

#[test]
fn refresh_paths_relinks_only_graph_diff_references_when_reference_ids_shift() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	let consumer = src.join("consumer.rs");
	let provider = src.join("provider.rs");
	fs::write(&lib, "pub mod consumer;\npub mod provider;\n").expect("write lib");
	fs::write(
		&consumer,
		"use crate::provider::stable;\npub fn call_provider() { stable(); }\n",
	)
	.expect("write consumer");
	fs::write(&provider, "pub fn stable() {}\n").expect("write provider");

	let cache = LocalResourceCache::default();
	let mut source_catalog = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![temp.path().to_path_buf()], None),
		cache.clone(),
	);
	let catalog = source_catalog
		.load_catalog(&WorkspaceRequest::new("incremental-diff-catalog"))
		.expect("catalog");
	let mut code_index = LocalCodeIndex::new(LocalCodeIndexOptions::new(None), cache.clone());
	let index = code_index.build_index(&catalog).expect("index");
	let mut linkage = LocalLinkage::new(cache);
	let linked = linkage
		.resolve_linkage_with_timings(&index)
		.expect("initial linkage");
	assert!(
		index_reference_resolves_to_symbol(&index, &linked.snapshot, "stable", "stable"),
		"`stable` should resolve before the incremental edit"
	);

	fs::write(
		&consumer,
		"use crate::provider::stable;\npub fn call_provider() { missing(); stable(); }\n",
	)
	.expect("rewrite consumer");
	let refreshed = code_index
		.refresh_paths(&index, std::slice::from_ref(&consumer))
		.expect("refresh paths");

	assert_eq!(refreshed.graph_diff.changed_symbol_count(), 0);
	assert_eq!(refreshed.graph_diff.changed_reference_count(), 1);
	assert!(
		!refreshed.graph_diff.reference_id_remaps.is_empty(),
		"the unchanged `stable` reference should be remapped after inserting `missing` before it"
	);

	let impact = LinkageRefreshImpact::with_graph_delta(
		refreshed.changed_sources.clone(),
		vec![consumer],
		LinkageGraphDelta::from_code_index(refreshed.graph_diff.clone()),
	);
	let refreshed_linkage = linkage
		.refresh_linkage_with_timings(&linked.snapshot, &refreshed.index, impact)
		.expect("incremental linkage refresh");

	assert_eq!(refreshed_linkage.timings.changed_refs, 1);
	assert!(
		index_reference_resolves_to_symbol(
			&refreshed.index,
			&refreshed_linkage.snapshot,
			"stable",
			"stable"
		),
		"`stable` should stay resolved through reference id remapping"
	);
	assert!(
		index_reference_is_unresolved(&refreshed.index, &refreshed_linkage.snapshot, "missing"),
		"only the inserted `missing` call should be unresolved"
	);
}

#[test]
fn refresh_paths_writes_changed_graphs_through_the_disk_cache() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	fs::write(&lib, "pub fn original() {}\n").expect("write lib");
	let cache_dir = tempfile::tempdir().expect("cache dir");

	let cache = LocalResourceCache::default();
	let mut source_catalog = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![temp.path().to_path_buf()], None),
		cache.clone(),
	);
	let catalog = source_catalog
		.load_catalog(&WorkspaceRequest::new("refresh-cache-catalog"))
		.expect("catalog");
	let mut code_index = LocalCodeIndex::new(
		LocalCodeIndexOptions::new(Some(cache_dir.path().to_path_buf())),
		cache,
	);
	let index = code_index.build_index(&catalog).expect("index");
	let entries_after_build = disk_cache_entries(cache_dir.path());
	assert!(
		!entries_after_build.is_empty(),
		"initial build should populate the disk cache"
	);

	fs::write(&lib, "pub fn original() {}\npub fn appended() {}\n").expect("rewrite lib");
	code_index
		.refresh_paths(&index, std::slice::from_ref(&lib))
		.expect("refresh paths");
	assert_ne!(
		disk_cache_entries(cache_dir.path()),
		entries_after_build,
		"refreshing a changed file should write its new graph through the disk cache"
	);
}

fn disk_cache_entries(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
	let mut entries = Vec::new();
	let Ok(dir_entries) = fs::read_dir(dir) else {
		return entries;
	};
	for entry in dir_entries.flatten() {
		let path = entry.path();
		if path.is_dir() {
			entries.extend(disk_cache_entries(&path));
		} else {
			let bytes = fs::read(&path).unwrap_or_default();
			entries.push((path, bytes));
		}
	}
	entries.sort();
	entries
}

#[test]
fn live_plan_indexes_created_files_without_a_rescan() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	fs::write(src.join("lib.rs"), "pub mod alpha;\npub mod fresh;\n").expect("lib");
	fs::write(src.join("alpha.rs"), "pub fn existing() {}\n").expect("alpha");
	let mut workspace = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
		vec![temp.path().to_path_buf()],
		None,
	));
	assert!(matches!(
		workspace
			.commands()
			.refresh(WorkspaceRequest::new("create-file-setup")),
		WorkspaceTransition::Ready { .. }
	));
	let baseline_symbols = workspace
		.queries()
		.snapshot()
		.expect("baseline snapshot")
		.index
		.symbols
		.len();

	let fresh = src.join("fresh.rs");
	fs::write(
		&fresh,
		"pub fn freshly_created() { crate::alpha::existing(); }\n",
	)
	.expect("fresh file");
	let plan =
		WorkspaceLiveRefreshPlan::from_event(WorkspaceLiveEvent::SourcesChanged(vec![fresh]));
	assert!(
		!plan.requires_rescan(),
		"a created source file should stay on the incremental plan"
	);
	let transition = workspace
		.live_commands()
		.apply_plan(WorkspaceRequest::new("create-file-live"), plan);
	assert!(matches!(
		transition.transition(),
		WorkspaceTransition::Ready { .. }
	));

	let snapshot = workspace.queries().snapshot().expect("snapshot");
	assert!(
		snapshot
			.index
			.symbols
			.iter()
			.any(|symbol| symbol.name == "freshly_created()"),
		"created file symbols should be indexed"
	);
	assert!(
		snapshot.index.symbols.len() > baseline_symbols,
		"symbol count should grow after the create"
	);
	assert!(
		snapshot
			.catalog
			.sources
			.iter()
			.any(|unit| unit.display_name.contains("fresh.rs")),
		"catalog should list the created file"
	);
	assert!(
		snapshot.linkage.resolved.iter().any(|edge| {
			snapshot.index.references.iter().any(|reference| {
				reference.id == edge.reference && reference.target_identity.contains("existing")
			})
		}),
		"the created file's call should resolve against the existing symbol"
	);
}

#[test]
fn refresh_paths_drops_removed_references_without_relinking_unchanged_graph() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	let consumer = src.join("consumer.rs");
	let provider = src.join("provider.rs");
	fs::write(&lib, "pub mod consumer;\npub mod provider;\n").expect("write lib");
	fs::write(
		&consumer,
		"use crate::provider::stable;\npub fn call_provider() { stable(); }\n",
	)
	.expect("write consumer");
	fs::write(&provider, "pub fn stable() {}\n").expect("write provider");

	let cache = LocalResourceCache::default();
	let mut source_catalog = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![temp.path().to_path_buf()], None),
		cache.clone(),
	);
	let catalog = source_catalog
		.load_catalog(&WorkspaceRequest::new("removed-reference-catalog"))
		.expect("catalog");
	let mut code_index = LocalCodeIndex::new(LocalCodeIndexOptions::new(None), cache.clone());
	let index = code_index.build_index(&catalog).expect("index");
	let mut linkage = LocalLinkage::new(cache);
	let linked = linkage
		.resolve_linkage_with_timings(&index)
		.expect("initial linkage");
	assert!(index_reference_resolves_to_symbol(
		&index,
		&linked.snapshot,
		"stable",
		"stable"
	));

	fs::write(&consumer, "pub fn call_provider() {}\n").expect("rewrite consumer");
	let refreshed = code_index
		.refresh_paths(&index, std::slice::from_ref(&consumer))
		.expect("refresh paths");

	assert_eq!(refreshed.graph_diff.changed_symbol_count(), 0);
	assert_eq!(refreshed.graph_diff.changed_references.len(), 0);
	assert!(
		!refreshed.graph_diff.removed_references.is_empty(),
		"the deleted `stable` call should be carried as a removed reference"
	);

	let impact = LinkageRefreshImpact::with_graph_delta(
		refreshed.changed_sources.clone(),
		vec![consumer],
		LinkageGraphDelta::from_code_index(refreshed.graph_diff.clone()),
	);
	let refreshed_linkage = linkage
		.refresh_linkage_with_timings(&linked.snapshot, &refreshed.index, impact)
		.expect("incremental linkage refresh");

	assert_eq!(refreshed_linkage.timings.changed_refs, 0);
	assert!(linkage_edges_reference_existing_records(
		&refreshed.index,
		&refreshed_linkage.snapshot
	));
	assert!(!index_reference_resolves_to_symbol(
		&refreshed.index,
		&refreshed_linkage.snapshot,
		"stable",
		"stable"
	));
}

#[test]
fn refresh_paths_relinks_existing_references_when_target_symbol_is_removed() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	let consumer = src.join("consumer.rs");
	let provider = src.join("provider.rs");
	fs::write(&lib, "pub mod consumer;\npub mod provider;\n").expect("write lib");
	fs::write(
		&consumer,
		"use crate::provider::stable;\npub fn call_provider() { stable(); }\n",
	)
	.expect("write consumer");
	fs::write(&provider, "pub fn stable() {}\n").expect("write provider");

	let cache = LocalResourceCache::default();
	let mut source_catalog = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![temp.path().to_path_buf()], None),
		cache.clone(),
	);
	let catalog = source_catalog
		.load_catalog(&WorkspaceRequest::new("removed-symbol-catalog"))
		.expect("catalog");
	let mut code_index = LocalCodeIndex::new(LocalCodeIndexOptions::new(None), cache.clone());
	let index = code_index.build_index(&catalog).expect("index");
	let mut linkage = LocalLinkage::new(cache);
	let linked = linkage
		.resolve_linkage_with_timings(&index)
		.expect("initial linkage");
	assert!(index_reference_resolves_to_symbol(
		&index,
		&linked.snapshot,
		"stable",
		"stable"
	));

	fs::write(&provider, "").expect("rewrite provider");
	let refreshed = code_index
		.refresh_paths(&index, std::slice::from_ref(&provider))
		.expect("refresh paths");

	assert_eq!(refreshed.graph_diff.changed_references.len(), 0);
	assert!(
		!refreshed.graph_diff.removed_symbols.is_empty(),
		"the deleted provider function should be carried as a removed symbol"
	);

	let impact = LinkageRefreshImpact::with_graph_delta(
		refreshed.changed_sources.clone(),
		vec![provider],
		LinkageGraphDelta::from_code_index(refreshed.graph_diff.clone()),
	);
	let refreshed_linkage = linkage
		.refresh_linkage_with_timings(&linked.snapshot, &refreshed.index, impact)
		.expect("incremental linkage refresh");

	assert!(
		refreshed_linkage.timings.changed_refs > 0,
		"references already resolved to the removed provider symbol should be relinked"
	);
	assert!(!index_reference_resolves_to_symbol(
		&refreshed.index,
		&refreshed_linkage.snapshot,
		"stable",
		"stable"
	));
	assert!(index_reference_is_unresolved(
		&refreshed.index,
		&refreshed_linkage.snapshot,
		"stable"
	));
}

#[test]
fn refresh_paths_does_not_relink_existing_references_for_added_target_symbol() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	let consumer = src.join("consumer.rs");
	let provider = src.join("provider.rs");
	fs::write(&lib, "pub mod consumer;\npub mod provider;\n").expect("write lib");
	fs::write(
		&consumer,
		"use crate::provider::stable;\npub fn call_provider() { stable(); }\n",
	)
	.expect("write consumer");
	fs::write(&provider, "pub fn stable() {}\n").expect("write provider");

	let cache = LocalResourceCache::default();
	let mut source_catalog = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![temp.path().to_path_buf()], None),
		cache.clone(),
	);
	let catalog = source_catalog
		.load_catalog(&WorkspaceRequest::new("added-symbol-catalog"))
		.expect("catalog");
	let mut code_index = LocalCodeIndex::new(LocalCodeIndexOptions::new(None), cache.clone());
	let index = code_index.build_index(&catalog).expect("index");
	let mut linkage = LocalLinkage::new(cache);
	let linked = linkage
		.resolve_linkage_with_timings(&index)
		.expect("initial linkage");
	assert!(index_reference_resolves_to_symbol(
		&index,
		&linked.snapshot,
		"stable",
		"stable"
	));

	fs::write(&provider, "pub fn stable() {}\npub fn added() {}\n").expect("rewrite provider");
	let refreshed = code_index
		.refresh_paths(&index, std::slice::from_ref(&provider))
		.expect("refresh paths");

	assert_eq!(refreshed.graph_diff.added_symbols.len(), 1);
	assert_eq!(refreshed.graph_diff.modified_symbols.len(), 0);
	assert_eq!(refreshed.graph_diff.removed_symbols.len(), 0);
	assert_eq!(refreshed.graph_diff.changed_references.len(), 0);

	let refreshed_linkage = linkage
		.refresh_linkage_with_timings(
			&linked.snapshot,
			&refreshed.index,
			linkage_impact(&refreshed, vec![provider]),
		)
		.expect("incremental linkage refresh");

	assert_eq!(refreshed_linkage.timings.stale_refs, 0);
	assert_eq!(refreshed_linkage.timings.changed_refs, 0);
	assert!(index_reference_resolves_to_symbol(
		&refreshed.index,
		&refreshed_linkage.snapshot,
		"stable",
		"stable"
	));
}

#[test]
fn refresh_paths_rebases_target_symbols_by_identity_after_removed_earlier_def() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	let consumer = src.join("consumer.rs");
	let provider = src.join("provider.rs");
	fs::write(&lib, "pub mod consumer;\npub mod provider;\n").expect("write lib");
	fs::write(
		&consumer,
		"use crate::provider::stable;\npub fn call_provider() { stable(); }\n",
	)
	.expect("write consumer");
	fs::write(&provider, "pub fn removed() {}\npub fn stable() {}\n").expect("write provider");

	let cache = LocalResourceCache::default();
	let mut source_catalog = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![temp.path().to_path_buf()], None),
		cache.clone(),
	);
	let catalog = source_catalog
		.load_catalog(&WorkspaceRequest::new("removed-earlier-symbol-catalog"))
		.expect("catalog");
	let mut code_index = LocalCodeIndex::new(LocalCodeIndexOptions::new(None), cache.clone());
	let index = code_index.build_index(&catalog).expect("index");
	let mut linkage = LocalLinkage::new(cache);
	let linked = linkage
		.resolve_linkage_with_timings(&index)
		.expect("initial linkage");
	assert!(index_reference_resolves_to_symbol(
		&index,
		&linked.snapshot,
		"stable",
		"stable"
	));

	fs::write(&provider, "pub fn stable() {}\n").expect("rewrite provider");
	let refreshed = code_index
		.refresh_paths(&index, std::slice::from_ref(&provider))
		.expect("refresh paths");

	assert_eq!(refreshed.graph_diff.removed_symbols.len(), 1);
	assert!(
		!refreshed.graph_diff.symbol_id_remaps.is_empty(),
		"`stable` should keep identity while its positional SymbolId shifts"
	);

	let refreshed_linkage = linkage
		.refresh_linkage_with_timings(
			&linked.snapshot,
			&refreshed.index,
			linkage_impact(&refreshed, vec![provider]),
		)
		.expect("incremental linkage refresh");

	assert_eq!(refreshed_linkage.timings.stale_refs, 0);
	assert!(index_reference_resolves_to_symbol(
		&refreshed.index,
		&refreshed_linkage.snapshot,
		"stable",
		"stable"
	));
	assert!(linkage_edges_reference_existing_records(
		&refreshed.index,
		&refreshed_linkage.snapshot
	));
}

#[test]
fn refresh_paths_rebases_roaring_ordinals_for_unchanged_later_files() {
	let temp = tempfile::tempdir().expect("tempdir");
	let src = temp.path().join("src");
	fs::create_dir_all(&src).expect("src dir");
	let lib = src.join("lib.rs");
	let alpha = src.join("a_consumer.rs");
	let beta = src.join("z_consumer.rs");
	let provider = src.join("provider.rs");
	fs::write(
		&lib,
		"pub mod a_consumer;\npub mod provider;\npub mod z_consumer;\n",
	)
	.expect("write lib");
	fs::write(
		&alpha,
		"use crate::provider::stable;\npub fn alpha_call() { stable(); }\n",
	)
	.expect("write alpha");
	fs::write(
		&beta,
		"use crate::provider::stable;\npub fn beta_call() { stable(); }\n",
	)
	.expect("write beta");
	fs::write(&provider, "pub fn stable() {}\n").expect("write provider");

	let cache = LocalResourceCache::default();
	let mut source_catalog = LocalSourceCatalog::new(
		LocalSourceCatalogOptions::new(vec![temp.path().to_path_buf()], None),
		cache.clone(),
	);
	let catalog = source_catalog
		.load_catalog(&WorkspaceRequest::new("ordinal-rebase-catalog"))
		.expect("catalog");
	let mut code_index = LocalCodeIndex::new(LocalCodeIndexOptions::new(None), cache.clone());
	let index = code_index.build_index(&catalog).expect("index");
	let mut linkage = LocalLinkage::new(cache);
	let linked = linkage
		.resolve_linkage_with_timings(&index)
		.expect("initial linkage");
	assert_eq!(
		index_resolved_reference_count(&index, &linked.snapshot, "calls", "stable", "stable"),
		2
	);

	fs::write(
		&alpha,
		"use crate::provider::stable;\npub fn alpha_call() { missing(); stable(); }\n",
	)
	.expect("rewrite alpha");
	let first_refresh = code_index
		.refresh_paths(&index, std::slice::from_ref(&alpha))
		.expect("first refresh paths");
	let first_linkage = linkage
		.refresh_linkage_with_timings(
			&linked.snapshot,
			&first_refresh.index,
			linkage_impact(&first_refresh, vec![alpha]),
		)
		.expect("first incremental linkage refresh");

	assert_eq!(first_linkage.timings.changed_refs, 1);
	assert_eq!(
		index_resolved_reference_count(
			&first_refresh.index,
			&first_linkage.snapshot,
			"calls",
			"stable",
			"stable"
		),
		2
	);
	assert_eq!(
		index_unresolved_reference_count(
			&first_refresh.index,
			&first_linkage.snapshot,
			"calls",
			"missing"
		),
		1
	);

	fs::write(&provider, "").expect("rewrite provider");
	let second_refresh = code_index
		.refresh_paths(&first_refresh.index, std::slice::from_ref(&provider))
		.expect("second refresh paths");
	let second_linkage = linkage
		.refresh_linkage_with_timings(
			&first_linkage.snapshot,
			&second_refresh.index,
			linkage_impact(&second_refresh, vec![provider]),
		)
		.expect("second incremental linkage refresh");

	assert!(second_linkage.timings.changed_refs >= 2);
	assert_eq!(
		index_resolved_reference_count(
			&second_refresh.index,
			&second_linkage.snapshot,
			"calls",
			"stable",
			"stable"
		),
		0
	);
	assert_eq!(
		index_unresolved_reference_count(
			&second_refresh.index,
			&second_linkage.snapshot,
			"calls",
			"stable"
		),
		2
	);
	assert!(linkage_edges_reference_existing_records(
		&second_refresh.index,
		&second_linkage.snapshot
	));
}

#[test]
fn registry_commands_publish_queryable_snapshots_and_scoped_events() {
	let options =
		LocalWorkspaceOptions::new(vec![fixture_path("projects/rust/multiproject")], None);
	let mut workspace = LocalWorkspaceRegistry::local(options);

	let before = workspace.events().event_cursor();
	let catalog = workspace
		.commands()
		.load_catalog(WorkspaceRequest::new("registry-catalog"));

	assert!(matches!(catalog, WorkspaceTransition::Ready { .. }));
	let catalog_snapshot = workspace.queries().snapshot().expect("catalog snapshot");
	assert!(!catalog_snapshot.catalog.sources.is_empty());
	assert!(catalog_snapshot.index.symbols.is_empty());
	assert_command_events(
		workspace.events().events_since(before),
		1,
		catalog_snapshot.generation.value(),
		&WorkspaceScopeUri::workspace(),
		&[
			WorkspaceEventKind::CommandAccepted,
			WorkspaceEventKind::WorkStarted,
			WorkspaceEventKind::SnapshotPublished,
			WorkspaceEventKind::WorkCompleted,
		],
	);

	let before = workspace.events().event_cursor();
	let index = workspace
		.commands()
		.load_index(WorkspaceRequest::new("registry-index").reuse_current_catalog());

	assert!(matches!(index, WorkspaceTransition::Ready { .. }));
	let index_snapshot = workspace.queries().snapshot().expect("index snapshot");
	assert!(!index_snapshot.index.symbols.is_empty());
	assert_eq!(index_snapshot.linkage.resolved_refs, 0);
	assert_command_events(
		workspace.events().events_since(before),
		2,
		index_snapshot.generation.value(),
		&WorkspaceScopeUri::workspace(),
		&[
			WorkspaceEventKind::CommandAccepted,
			WorkspaceEventKind::WorkStarted,
			WorkspaceEventKind::SnapshotPublished,
			WorkspaceEventKind::WorkCompleted,
		],
	);

	let before = workspace.events().event_cursor();
	let linkage = workspace
		.commands()
		.resolve_linkage(WorkspaceRequest::new("registry-linkage"));

	assert!(matches!(linkage, WorkspaceTransition::Ready { .. }));
	let linked_snapshot = workspace.queries().snapshot().expect("linked snapshot");
	assert!(linked_snapshot.linkage.resolved_refs > 0);
	assert_command_events(
		workspace.events().events_since(before),
		3,
		linked_snapshot.generation.value(),
		&WorkspaceScopeUri::workspace(),
		&[
			WorkspaceEventKind::CommandAccepted,
			WorkspaceEventKind::WorkStarted,
			WorkspaceEventKind::SnapshotPublished,
			WorkspaceEventKind::WorkCompleted,
		],
	);
}

#[test]
fn live_plan_falls_back_to_workspace_rescan_and_reports_watcher_replacement() {
	let temp = tempfile::tempdir().expect("tempdir");
	let source = temp.path().join("src").join("lib.rs");
	fs::create_dir_all(source.parent().expect("source parent")).expect("src dir");
	fs::write(&source, "pub fn live_surface() {}\n").expect("write source");
	let mut workspace = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
		vec![temp.path().to_path_buf()],
		None,
	));
	let plan =
		WorkspaceLiveRefreshPlan::from_event(WorkspaceLiveEvent::SourcesChanged(vec![source]));

	let live = workspace
		.live_commands()
		.apply_plan(WorkspaceRequest::new("live-plan"), plan);
	let replace_watcher = live.replace_watcher();

	assert!(matches!(
		live.transition(),
		WorkspaceTransition::Ready { .. }
	));
	assert!(replace_watcher);
	let snapshot = workspace.queries().snapshot().expect("live snapshot");
	assert!(
		snapshot
			.index
			.symbols
			.iter()
			.any(|symbol| symbol.identity.contains("live_surface"))
	);
}

#[test]
fn workspace_live_watcher_tracks_startup_roots_recursively() {
	let temp = tempfile::tempdir().expect("tempdir");
	let source_dir = temp.path().join("src").join("nested");
	let mut workspace = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
		vec![temp.path().to_path_buf()],
		None,
	));

	assert!(matches!(
		workspace
			.commands()
			.load_index(WorkspaceRequest::new("acceptance-index")),
		WorkspaceTransition::Ready { .. }
	));

	let watch_roots = workspace.watch_roots();
	let expected_root = temp.path().canonicalize().expect("canonical root");
	let unexpected_child = source_dir.clone();
	assert!(
		watch_roots.iter().any(|root| root.path == expected_root),
		"workspace should expose the startup root {}",
		expected_root.display()
	);
	assert!(
		!watch_roots.iter().any(|root| root.path == unexpected_child),
		"workspace should not expose recursively discovered child roots like {}",
		unexpected_child.display()
	);

	let (tx, rx) = mpsc::channel();
	let _watcher = LiveWorkspaceWatcher::start_polling(watch_roots, move |event| {
		let _ = tx.send(event);
	})
	.expect("watcher starts");
	std::thread::sleep(Duration::from_millis(200));

	fs::create_dir_all(&source_dir).expect("nested source dir");
	let changed_source = source_dir.join("changed.rs");
	fs::write(&changed_source, "pub fn changed() {}\n").expect("write changed source");

	let event = rx
		.recv_timeout(Duration::from_secs(5))
		.expect("recursive watcher should publish nested source change");
	assert!(
		matches!(
			event,
			WorkspaceLiveEvent::SourcesChanged(_) | WorkspaceLiveEvent::RescanRequired
		),
		"unexpected live event for nested source change: {event:?}"
	);
}

#[cfg(unix)]
#[test]
fn workspace_live_watcher_starts_when_git_contains_fsmonitor_socket() {
	let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("tempdir");
	let git_dir = temp.path().join(".git");
	let source_dir = temp.path().join("src");
	fs::create_dir_all(&git_dir).expect("git dir");
	fs::create_dir_all(&source_dir).expect("source dir");
	let source = source_dir.join("lib.rs");
	fs::write(&source, "pub fn git_socket_root() {}\n").expect("write source");
	let socket_path = git_dir.join("fsmonitor--daemon.ipc");
	let _socket = std::os::unix::net::UnixListener::bind(&socket_path).expect("bind git socket");
	let mut workspace = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
		vec![temp.path().to_path_buf()],
		None,
	));

	assert!(matches!(
		workspace
			.commands()
			.load_index(WorkspaceRequest::new("acceptance-index")),
		WorkspaceTransition::Ready { .. }
	));

	let (tx, rx) = mpsc::channel();
	let watcher = LiveWorkspaceWatcher::start_polling(workspace.watch_roots(), move |event| {
		let _ = tx.send(event);
	})
	.expect("watcher should start for repository root");
	let status = watcher.status().unwrap_or_default();
	assert!(
		!status.contains("disabled"),
		"git fsmonitor socket should not disable live watcher: {status}"
	);
	std::thread::sleep(Duration::from_millis(200));

	fs::write(
		&source,
		"pub fn git_socket_root() {}\npub fn git_socket_refresh() {}\n",
	)
	.expect("rewrite source");

	let event = rx
		.recv_timeout(Duration::from_secs(3))
		.expect("recursive watcher should survive git socket and publish source change");
	assert!(
		matches!(
			event,
			WorkspaceLiveEvent::SourcesChanged(_) | WorkspaceLiveEvent::RescanRequired
		),
		"unexpected live event with git socket present: {event:?}"
	);
}

#[test]
fn registry_commands_report_failure_and_keep_scope_uri() {
	let options =
		LocalWorkspaceOptions::new(vec![fixture_path("projects/rust/multiproject")], None);
	let mut workspace = LocalWorkspaceRegistry::local(options);
	let scope = WorkspaceScopeUri::new("code+moniker://./dir:projects/dir:rust");

	let before = workspace.events().event_cursor();
	let transition = workspace.commands().execute(WorkspaceCommandSpec::new(
		WorkspaceCommandKind::ResolveLinkage,
		scope.clone(),
		WorkspaceRequest::new("linkage-before-index"),
	));

	let WorkspaceTransition::Failed {
		failure,
		preserved_generation,
	} = transition
	else {
		panic!("resolve linkage without index should fail");
	};
	assert_eq!(failure.resource, WorkspaceResource::LinkageSnapshot);
	assert!(preserved_generation.is_none());
	assert!(workspace.queries().snapshot().is_none());
	assert_eq!(workspace.queries().last_failure(), Some(&failure));
	assert_command_events(
		workspace.events().events_since(before),
		1,
		1,
		&scope,
		&[
			WorkspaceEventKind::CommandAccepted,
			WorkspaceEventKind::WorkStarted,
			WorkspaceEventKind::WorkFailed,
		],
	);
}

#[test]
fn registry_adopts_worker_snapshots_through_command_events() {
	let options =
		LocalWorkspaceOptions::new(vec![fixture_path("projects/rust/multiproject")], None);
	let mut worker = LocalWorkspaceRegistry::local(options.clone());
	assert!(matches!(
		worker
			.commands()
			.load_index(WorkspaceRequest::new("worker-index")),
		WorkspaceTransition::Ready { .. }
	));
	let snapshot = worker
		.queries()
		.snapshot_arc()
		.expect("worker snapshot should be queryable");

	let mut owner = LocalWorkspaceRegistry::local(options);
	let before = owner.events().event_cursor();
	let transition = owner
		.commands()
		.publish_snapshot(WorkspaceSnapshotPublication::workspace(
			WorkspaceRequest::new("worker-result"),
			snapshot.clone(),
		));

	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	assert_eq!(
		owner
			.queries()
			.snapshot()
			.map(|snapshot| snapshot.generation),
		Some(snapshot.generation)
	);
	assert_command_events(
		owner.events().events_since(before),
		1,
		snapshot.generation.value(),
		&WorkspaceScopeUri::workspace(),
		&[
			WorkspaceEventKind::CommandAccepted,
			WorkspaceEventKind::WorkStarted,
			WorkspaceEventKind::SnapshotPublished,
			WorkspaceEventKind::WorkCompleted,
		],
	);
}

fn assert_command_events(
	events: &[code_moniker_workspace::WorkspaceEvent],
	command_id: u64,
	generation: u64,
	scope: &WorkspaceScopeUri,
	expected_kinds: &[WorkspaceEventKind],
) {
	let kinds = events.iter().map(|event| event.kind).collect::<Vec<_>>();
	assert_eq!(kinds, expected_kinds);
	assert!(events.iter().all(|event| &event.scope_uri == scope));
	assert!(
		events
			.iter()
			.all(|event| event.command_id.value() == command_id)
	);
	assert!(
		events
			.iter()
			.all(|event| event.generation.value() == generation)
	);
}

fn reference_resolves_to_symbol(
	snapshot: &code_moniker_workspace::snapshot::WorkspaceSnapshot,
	reference_target: &str,
	symbol_identity: &str,
) -> bool {
	snapshot
		.index
		.references
		.iter()
		.filter(|reference| reference.target_identity.contains(reference_target))
		.any(|reference| {
			snapshot
				.linkage
				.resolved
				.iter()
				.filter(|edge| edge.reference == reference.id)
				.any(|edge| {
					snapshot.index.symbols.iter().any(|symbol| {
						symbol.id == edge.target && symbol.identity.contains(symbol_identity)
					})
				})
		})
}

fn index_reference_resolves_to_symbol(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	reference_target: &str,
	symbol_identity: &str,
) -> bool {
	index
		.references
		.iter()
		.filter(|reference| reference.target_identity.contains(reference_target))
		.any(|reference| {
			linkage
				.resolved
				.iter()
				.filter(|edge| edge.reference == reference.id)
				.any(|edge| {
					index.symbols.iter().any(|symbol| {
						symbol.id == edge.target && symbol.identity.contains(symbol_identity)
					})
				})
		})
}

fn index_resolved_reference_count(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	kind: &str,
	reference_target: &str,
	symbol_identity: &str,
) -> usize {
	index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == kind && reference.target_identity.contains(reference_target)
		})
		.filter(|reference| {
			linkage
				.resolved
				.iter()
				.filter(|edge| edge.reference == reference.id)
				.any(|edge| {
					index.symbols.iter().any(|symbol| {
						symbol.id == edge.target && symbol.identity.contains(symbol_identity)
					})
				})
		})
		.count()
}

fn index_reference_is_unresolved(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	reference_target: &str,
) -> bool {
	index
		.references
		.iter()
		.filter(|reference| reference.target_identity.contains(reference_target))
		.any(|reference| {
			linkage
				.unresolved
				.iter()
				.chain(linkage.manifest_blocked.iter())
				.any(|unresolved| unresolved.reference == reference.id)
		})
}

fn index_unresolved_reference_count(
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	kind: &str,
	reference_target: &str,
) -> usize {
	index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == kind && reference.target_identity.contains(reference_target)
		})
		.filter(|reference| {
			linkage
				.unresolved
				.iter()
				.chain(linkage.manifest_blocked.iter())
				.any(|unresolved| unresolved.reference == reference.id)
		})
		.count()
}

fn linkage_impact(refreshed: &CodeIndexRefresh, paths: Vec<PathBuf>) -> LinkageRefreshImpact {
	LinkageRefreshImpact::with_graph_delta(
		refreshed.changed_sources.clone(),
		paths,
		LinkageGraphDelta::from_code_index(refreshed.graph_diff.clone()),
	)
}

fn linkage_edges_reference_existing_records(index: &CodeIndex, linkage: &LinkageSnapshot) -> bool {
	linkage.resolved.iter().all(|edge| {
		index
			.references
			.iter()
			.any(|reference| reference.id == edge.reference)
	}) && linkage.external.iter().all(|edge| {
		index
			.references
			.iter()
			.any(|reference| reference.id == edge.reference)
	}) && linkage.manifest_blocked.iter().all(|edge| {
		index
			.references
			.iter()
			.any(|reference| reference.id == edge.reference)
	}) && linkage.unresolved.iter().all(|edge| {
		index
			.references
			.iter()
			.any(|reference| reference.id == edge.reference)
	})
}

fn reference_is_external(
	snapshot: &code_moniker_workspace::snapshot::WorkspaceSnapshot,
	reference_target: &str,
) -> bool {
	snapshot
		.index
		.references
		.iter()
		.filter(|reference| reference.target_identity.contains(reference_target))
		.any(|reference| {
			snapshot
				.linkage
				.external
				.iter()
				.any(|external| external.reference == reference.id)
		})
}

fn reference_is_unresolved(
	snapshot: &code_moniker_workspace::snapshot::WorkspaceSnapshot,
	reference_target: &str,
) -> bool {
	snapshot
		.index
		.references
		.iter()
		.filter(|reference| reference.target_identity.contains(reference_target))
		.any(|reference| {
			snapshot
				.linkage
				.unresolved
				.iter()
				.chain(snapshot.linkage.manifest_blocked.iter())
				.any(|unresolved| unresolved.reference == reference.id)
		})
}
