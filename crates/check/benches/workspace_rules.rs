//! Baseline and bitmap-plan benchmarks for workspace rules.

mod support;

use std::collections::BTreeMap;
use std::fs;

use code_moniker_check::{
	DefaultRulesSelection, RuleSetRequest, compile_workspace_rules, evaluate_workspace_rules,
	workspace::{WorkspaceCheckRunner, WorkspaceCheckRunnerOptions, WorkspaceEvaluationMode},
};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{
	CodeIndex, WorkspaceRequest, WorkspaceSnapshot, WorkspaceTransition,
};
use code_moniker_workspace::source::LocalResourceCache;
use criterion::{Criterion, criterion_group, criterion_main};

const MODULES: usize = 120;
const SYMBOLS_PER_MODULE: usize = 40;

fn indexed_workspace(
	workspace: &support::SyntheticWorkspace,
) -> (
	LocalWorkspaceRegistry,
	std::sync::Arc<WorkspaceSnapshot>,
	LocalResourceCache,
) {
	let cache = LocalResourceCache::default();
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![workspace.root().to_path_buf()], None),
		cache.clone(),
	);
	let transition = registry
		.commands()
		.refresh(WorkspaceRequest::new("workspace-rule-bench"));
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let snapshot = registry
		.queries()
		.snapshot_arc()
		.expect("workspace-rule benchmark snapshot");
	(registry, snapshot, cache)
}

fn naive_repository_violations(index: &CodeIndex) -> usize {
	index
		.symbols
		.iter()
		.filter(|symbol| {
			symbol.kind == "struct"
				&& symbol.name.ends_with("Repository")
				&& !symbol
					.identity
					.split('/')
					.any(|segment| segment == "dir:infra")
		})
		.count()
}

fn naive_name_collisions(index: &CodeIndex) -> usize {
	let mut counts = BTreeMap::<(String, String, String), usize>::new();
	for symbol in index
		.symbols
		.iter()
		.filter(|symbol| symbol.kind == "struct")
	{
		let source = &index.sources[symbol.source.file()];
		let owner = symbol
			.identity
			.split('/')
			.filter(|segment| segment.starts_with("package:") || segment.starts_with("dir:"))
			.collect::<Vec<_>>()
			.join("/");
		*counts
			.entry((source.language.clone(), owner, symbol.name.clone()))
			.or_default() += 1;
	}
	counts.values().filter(|count| **count > 1).count()
}

fn scan_baseline(c: &mut Criterion) {
	let workspace = support::generate(MODULES, SYMBOLS_PER_MODULE);
	let (_registry, snapshot, _cache) = indexed_workspace(&workspace);
	assert!(naive_repository_violations(&snapshot.index) > 0);
	assert!(naive_name_collisions(&snapshot.index) > 0);
	let cfg = RuleSetRequest::new(None, "code+moniker://")
		.with_default_rules(DefaultRulesSelection::Disabled)
		.with_inline_rules(vec![
			r#"
			[[workspace.symbol.where]]
			id = "repositories-under-infra"
			expr = "(shape = 'type' AND name =~ Repository$) => uri ~ '**/dir:infra/**'"
			"#
			.to_string(),
		])
		.load_config()
		.expect("workspace benchmark config");
	let compiled =
		compile_workspace_rules(&cfg, "code+moniker://").expect("workspace benchmark plan");
	assert_eq!(
		evaluate_workspace_rules(&snapshot.index.inventory, &compiled, false)
			.violations
			.len(),
		naive_repository_violations(&snapshot.index)
	);
	let group_cfg = RuleSetRequest::new(None, "code+moniker://")
		.with_default_rules(DefaultRulesSelection::Disabled)
		.with_inline_rules(vec![
			r#"
			[[workspace.group.where]]
			id = "unique-type-name"
			members = "shape = 'type'"
			group_by = ["lang", "segment('dir')", "name"]
			expr = "count(member) <= 1"
			"#
			.to_string(),
		])
		.load_config()
		.expect("workspace group benchmark config");
	let compiled_groups =
		compile_workspace_rules(&group_cfg, "code+moniker://").expect("workspace group plan");
	let group_evaluation =
		evaluate_workspace_rules(&snapshot.index.inventory, &compiled_groups, false);
	assert_eq!(
		group_evaluation
			.groups
			.iter()
			.filter(|group| !group.passed)
			.count(),
		naive_name_collisions(&snapshot.index)
	);
	let mut group = c.benchmark_group("workspace_rules_baseline");
	group.bench_function("placement_full_scan", |b| {
		b.iter(|| {
			std::hint::black_box(naive_repository_violations(std::hint::black_box(
				&snapshot.index,
			)))
		});
	});
	group.bench_function("grouping_full_scan", |b| {
		b.iter(|| {
			std::hint::black_box(naive_name_collisions(std::hint::black_box(&snapshot.index)))
		});
	});
	group.bench_function("placement_bitmap_plan", |b| {
		b.iter(|| {
			std::hint::black_box(evaluate_workspace_rules(
				std::hint::black_box(&snapshot.index.inventory),
				std::hint::black_box(&compiled),
				false,
			))
		});
	});
	group.bench_function("grouping_bitmap_plan", |b| {
		b.iter(|| {
			std::hint::black_box(evaluate_workspace_rules(
				std::hint::black_box(&snapshot.index.inventory),
				std::hint::black_box(&compiled_groups),
				false,
			))
		});
	});
	group.finish();
}

fn refresh_baseline(c: &mut Criterion) {
	let workspace = support::generate(MODULES, SYMBOLS_PER_MODULE);
	let (mut registry, _, _) = indexed_workspace(&workspace);
	let changed_module = workspace.changed_module();
	let mut salt = 0usize;
	let mut group = c.benchmark_group("workspace_rules_baseline");
	group.sample_size(20);
	group.bench_function("refresh_then_full_scan", |b| {
		b.iter(|| {
			salt += 1;
			workspace.rewrite_module(changed_module, salt);
			let transition = registry.commands().refresh_paths(
				WorkspaceRequest::new("workspace-rule-refresh"),
				vec![workspace.root().join(format!(
					"src/{}/m{changed_module}.rs",
					if changed_module % 2 == 0 {
						"infra"
					} else {
						"domain"
					}
				))],
			);
			assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
			let snapshot = registry
				.queries()
				.snapshot()
				.expect("refreshed workspace-rule snapshot");
			std::hint::black_box(naive_repository_violations(&snapshot.index));
			std::hint::black_box(naive_name_collisions(&snapshot.index));
		});
	});
	group.finish();

	let workspace = support::generate(MODULES, SYMBOLS_PER_MODULE);
	let rules = workspace.root().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[workspace.symbol.where]]
id = "repositories-under-infra"
expr = "(shape = 'type' AND name =~ Repository$) => uri ~ '**/dir:infra/**'"

[[workspace.group.where]]
id = "unique-type-name"
members = "shape = 'type'"
group_by = ["lang", "segment('dir')", "name"]
expr = "count(member) <= 1"
"#,
	)
	.expect("incremental benchmark rules");
	let (mut registry, initial, cache) = indexed_workspace(&workspace);
	let mut runner = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(rules, None, "code+moniker://"),
		cache,
	);
	runner
		.run_check(&initial.index, &initial.linkage)
		.expect("seed incremental workspace-rule evaluation");
	let changed_module = workspace.changed_module();
	let mut salt = 0usize;
	let mut group = c.benchmark_group("workspace_rules_incremental");
	group.sample_size(20);
	group.bench_function("refresh_then_incremental_rules", |b| {
		b.iter(|| {
			salt += 1;
			workspace.rewrite_module(changed_module, salt);
			let transition = registry.commands().refresh_paths(
				WorkspaceRequest::new("workspace-rule-incremental-refresh"),
				vec![
					workspace
						.root()
						.join(format!("src/infra/m{changed_module}.rs")),
				],
			);
			assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
			let snapshot = registry
				.queries()
				.snapshot()
				.expect("incremental workspace-rule snapshot");
			let diagnostics = runner
				.run_check(&snapshot.index, &snapshot.linkage)
				.expect("incremental workspace-rule evaluation");
			assert_eq!(
				diagnostics.evaluation.mode,
				WorkspaceEvaluationMode::Incremental
			);
			std::hint::black_box(diagnostics);
		});
	});
	group.finish();
}

criterion_group!(benches, scan_baseline, refresh_baseline);
criterion_main!(benches);
