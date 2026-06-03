use std::fs;
use std::path::{Path, PathBuf};

use code_moniker_workspace::registry::{
	LocalWorkspaceOptions, LocalWorkspaceRegistry, WorkspaceCommandKind, WorkspaceCommandSpec,
	WorkspaceEventKind, WorkspaceScopeUri, WorkspaceSnapshotPublication,
};
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceResource, WorkspaceTransition};

fn fixture_path(path: impl AsRef<Path>) -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures")
		.join(path)
}

#[test]
fn refresh_paths_garbage_collects_stale_unresolved_refs_when_provider_symbols_change() {
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
fn refresh_paths_garbage_collects_stale_manifest_policy_refs_when_manifest_changes() {
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
		"`zustand/create` stale unresolved projection should be garbage collected"
	);
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
	assert_eq!(failure.resource, WorkspaceResource::LinkageGraph);
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
