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
