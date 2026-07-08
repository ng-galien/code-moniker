//! Indexing benchmarks: full build, single-file refresh, and the
//! create/remove-file paths (today a full rescan; the catalog-delta work is
//! expected to pull them down to content-edit cost).

mod support;

use std::fs;

use code_moniker_workspace::live::{WorkspaceLiveEvent, WorkspaceLiveRefreshPlan};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceTransition};
use criterion::{Criterion, criterion_group, criterion_main};

const MODULES: usize = 120;
const FNS_PER_MODULE: usize = 10;

fn indexed_registry(workspace: &support::SyntheticWorkspace) -> LocalWorkspaceRegistry {
	let mut registry = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
		vec![workspace.root().to_path_buf()],
		None,
	));
	registry
		.commands()
		.refresh(WorkspaceRequest::new("bench-setup"));
	assert!(
		registry.queries().snapshot().is_some(),
		"bench setup build failed: {:?}",
		registry.queries().last_failure()
	);
	registry
}

fn full_build(c: &mut Criterion) {
	let workspace = support::generate(MODULES, FNS_PER_MODULE);
	let mut group = c.benchmark_group("indexing");
	group.sample_size(10);
	group.bench_function("full_build", |b| {
		b.iter(|| {
			let mut registry = LocalWorkspaceRegistry::local(LocalWorkspaceOptions::new(
				vec![workspace.root().to_path_buf()],
				None,
			));
			registry
				.commands()
				.refresh(WorkspaceRequest::new("bench-full"));
			assert!(registry.queries().snapshot().is_some());
		});
	});
	group.finish();
}

fn refresh_edit_one_file(c: &mut Criterion) {
	let workspace = support::generate(MODULES, FNS_PER_MODULE);
	let mut registry = indexed_registry(&workspace);
	let mut group = c.benchmark_group("indexing");
	group.sample_size(20);
	let mut salt = 0usize;
	group.bench_function("refresh_edit_one_file", |b| {
		b.iter(|| {
			salt += 1;
			workspace.rewrite_module(3, salt);
			let plan =
				WorkspaceLiveRefreshPlan::from_event(WorkspaceLiveEvent::SourcesChanged(vec![
					workspace.module_path(3),
				]));
			let transition = registry
				.live_commands()
				.apply_plan(WorkspaceRequest::new("bench-edit"), plan);
			assert!(matches!(
				transition.transition(),
				WorkspaceTransition::Ready { .. }
			));
		});
	});
	group.finish();
}

fn refresh_create_and_remove_file(c: &mut Criterion) {
	let workspace = support::generate(MODULES, FNS_PER_MODULE);
	let mut registry = indexed_registry(&workspace);
	let extra = workspace.root().join("src/extra.rs");
	let mut group = c.benchmark_group("indexing");
	group.sample_size(10);
	group.bench_function("refresh_create_then_remove_file", |b| {
		b.iter(|| {
			fs::write(&extra, "pub fn extra_probe() -> u32 {\n\t42\n}\n").expect("create");
			let create = registry.live_commands().apply_plan(
				WorkspaceRequest::new("bench-create"),
				WorkspaceLiveRefreshPlan::from_event(WorkspaceLiveEvent::RescanRequired),
			);
			assert!(matches!(
				create.transition(),
				WorkspaceTransition::Ready { .. }
			));
			fs::remove_file(&extra).expect("remove");
			let remove = registry.live_commands().apply_plan(
				WorkspaceRequest::new("bench-remove"),
				WorkspaceLiveRefreshPlan::from_event(WorkspaceLiveEvent::RescanRequired),
			);
			assert!(matches!(
				remove.transition(),
				WorkspaceTransition::Ready { .. }
			));
		});
	});
	group.finish();
}

fn snapshot_clone(c: &mut Criterion) {
	let workspace = support::generate(MODULES, FNS_PER_MODULE);
	let registry = indexed_registry(&workspace);
	let snapshot = registry
		.queries()
		.snapshot_arc()
		.expect("snapshot for clone bench");
	let mut group = c.benchmark_group("indexing");
	group.bench_function("snapshot_clone", |b| {
		b.iter(|| std::hint::black_box((*snapshot).clone()));
	});
	group.finish();
}

criterion_group!(
	benches,
	full_build,
	refresh_edit_one_file,
	refresh_create_and_remove_file,
	snapshot_clone
);
criterion_main!(benches);
