use std::fs;
use std::path::Path;

use code_moniker_check::workspace::{
	WorkspaceCheckRunner, WorkspaceCheckRunnerOptions, WorkspaceEvaluationMode,
};
use code_moniker_check::{CheckRequest, DefaultRulesSelection, RuleSetRequest};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceTransition};
use code_moniker_workspace::source::LocalResourceCache;

const SCHEME: &str = "code+moniker://";
const RULE_ID: &str = "workspace.group.unique-type-name-per-package";
const STATISTIC_RULE_ID: &str = "workspace.group.balanced-invoice-size";

fn write(root: &Path, path: &str, source: &str) {
	let target = root.join(path);
	fs::create_dir_all(target.parent().expect("fixture parent")).expect("fixture directory");
	fs::write(target, source).expect("fixture source");
}

fn group_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	for (path, package, container, member) in [
		(
			"src/main/java/com/acme/sales/SalesA.java",
			"com.acme.sales",
			"SalesA",
			"Invoice",
		),
		(
			"src/main/java/com/acme/sales/SalesB.java",
			"com.acme.sales",
			"SalesB",
			"Invoice",
		),
		(
			"src/main/java/com/acme/legacy/LegacyA.java",
			"com.acme.legacy",
			"LegacyA",
			"Legacy",
		),
		(
			"src/main/java/com/acme/legacy/LegacyB.java",
			"com.acme.legacy",
			"LegacyB",
			"Legacy",
		),
	] {
		write(
			fixture.path(),
			path,
			&format!("package {package};\n\nclass {container} {{\n\tclass {member} {{}}\n}}\n"),
		);
	}
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[workspace.group.where]]
id = "unique-type-name-per-package"
severity = "warn"
members = "shape = 'type'"
group_by = ["lang", "segment('package')", "name"]
expr = "count(member) <= 1"
message = "Duplicate type group {group}: {members}"
suppress = [{ values = ["java", "com.acme.legacy", "Legacy"] }]
"#,
	)
	.expect("rules");
	(fixture, rules)
}

fn assert_skipped_generation_falls_back(
	root: &Path,
	rules: &Path,
	registry: &mut LocalWorkspaceRegistry,
	runner: &mut WorkspaceCheckRunner,
	cache: &LocalResourceCache,
) {
	let source_path = root.join("src/main/java/com/acme/sales/SalesA.java");
	for method in ["first", "second"] {
		write(
			root,
			"src/main/java/com/acme/sales/SalesA.java",
			&format!(
				"package com.acme.sales;\n\nclass SalesA {{\n\tclass Invoice {{}}\n\tvoid {method}() {{}}\n}}\n"
			),
		);
		let transition = registry.commands().refresh_paths(
			WorkspaceRequest::new("workspace-group-skipped-generation"),
			vec![source_path.clone()],
		);
		assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	}
	let skipped = registry
		.queries()
		.snapshot()
		.expect("snapshot after skipped generation");
	let fallback = runner
		.run_check(&skipped.index, &skipped.linkage)
		.expect("safe full fallback");
	let mut cold = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(rules.to_path_buf(), None, SCHEME),
		cache.clone(),
	);
	let expected = cold
		.run_check(&skipped.index, &skipped.linkage)
		.expect("cold result after skipped generation");
	assert_eq!(fallback.diagnostics, expected.diagnostics);
	assert_eq!(fallback.evaluation.mode, WorkspaceEvaluationMode::Full);
}

#[test]
fn group_collision_is_single_stable_diagnostic_across_one_shot_and_snapshot() {
	let (fixture, rules) = group_fixture();
	let specs = RuleSetRequest::with_rules(&rules, SCHEME)
		.with_default_rules(DefaultRulesSelection::Disabled)
		.compiled_specs_for_langs(std::iter::empty())
		.expect("workspace group specs");
	assert_eq!(specs.len(), 1);
	assert_eq!(specs[0].root, "workspace");
	assert_eq!(specs[0].subject, "group");
	assert_eq!(specs[0].plan, "t1_inventory");

	let one_shot = CheckRequest::new(
		fixture.path(),
		RuleSetRequest::with_rules(&rules, SCHEME)
			.with_default_rules(DefaultRulesSelection::Disabled),
	)
	.run()
	.expect("one-shot group check");
	let violations = one_shot.file_violations().collect::<Vec<_>>();
	assert_eq!(violations.len(), 1, "{violations:#?}");
	assert_eq!(violations[0].1.rule_id, RULE_ID);
	assert!(violations[0].0.ends_with("SalesA.java"));
	assert!(violations[0].1.message.contains("com.acme.sales"));
	assert!(violations[0].1.message.contains("2 members"));

	let cache = LocalResourceCache::default();
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![fixture.path().to_path_buf()], None),
		cache.clone(),
	);
	let transition = registry
		.commands()
		.refresh(WorkspaceRequest::new("workspace-group-runner"));
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let snapshot = registry.queries().snapshot().expect("snapshot");
	let mut runner = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(rules.clone(), None, SCHEME),
		cache.clone(),
	);
	let diagnostics = runner
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("snapshot group check");
	let group_diagnostics = diagnostics
		.diagnostics
		.iter()
		.filter(|diagnostic| diagnostic.rule_id == RULE_ID)
		.collect::<Vec<_>>();
	assert_eq!(group_diagnostics.len(), 1, "{group_diagnostics:#?}");
	let primary = group_diagnostics[0]
		.symbol
		.expect("group diagnostic primary symbol");
	assert_eq!(
		snapshot
			.index
			.symbols
			.file_records(primary.file())
			.get(primary.def())
			.expect("primary group member")
			.name,
		"Invoice"
	);

	write(
		fixture.path(),
		"src/main/java/com/acme/sales/SalesB.java",
		"package com.acme.orders;\n\nclass SalesB {\n\tclass Invoice {}\n}\n",
	);
	let transition = registry.commands().refresh_paths(
		WorkspaceRequest::new("workspace-group-move"),
		vec![
			fixture
				.path()
				.join("src/main/java/com/acme/sales/SalesB.java"),
		],
	);
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let moved = registry.queries().snapshot().expect("moved snapshot");
	let incremental = runner
		.run_check(&moved.index, &moved.linkage)
		.expect("incremental group check");
	let mut cold_runner = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(rules.clone(), None, SCHEME),
		cache.clone(),
	);
	let cold = cold_runner
		.run_check(&moved.index, &moved.linkage)
		.expect("cold group check");
	assert_eq!(incremental.diagnostics, cold.diagnostics);
	assert_eq!(
		incremental.evaluation.mode,
		WorkspaceEvaluationMode::Incremental
	);
	assert_eq!(incremental.evaluation.dirty_symbols, 6);
	assert_eq!(incremental.evaluation.evaluated_symbols, 3);
	assert_eq!(incremental.evaluation.affected_groups, 4);
	assert!(incremental.evaluation.evaluated_symbols < moved.index.inventory.all_symbols().len());
	assert_skipped_generation_falls_back(
		fixture.path(),
		&rules,
		&mut registry,
		&mut runner,
		&cache,
	);
}

#[test]
fn changing_group_suppression_invalidates_cached_evaluation() {
	let (fixture, rules) = group_fixture();
	let cache = LocalResourceCache::default();
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![fixture.path().to_path_buf()], None),
		cache.clone(),
	);
	let transition = registry
		.commands()
		.refresh(WorkspaceRequest::new("workspace-group-suppression-seed"));
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let snapshot = registry.queries().snapshot().expect("snapshot");
	let mut runner = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(rules.clone(), None, SCHEME),
		cache.clone(),
	);
	let initial = runner
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("initial group evaluation");
	assert_eq!(
		initial
			.diagnostics
			.iter()
			.filter(|diagnostic| diagnostic.rule_id == RULE_ID)
			.count(),
		1
	);

	let updated = fs::read_to_string(&rules)
		.expect("group rules")
		.replace(
			"suppress = [{ values = [\"java\", \"com.acme.legacy\", \"Legacy\"] }]",
			"suppress = [{ values = [\"java\", \"com.acme.legacy\", \"Legacy\"] }, { values = [\"java\", \"com.acme.sales\", \"Invoice\"] }]",
		);
	fs::write(&rules, updated).expect("updated group suppression");
	let cached = runner
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("evaluation after suppression change");
	let mut cold =
		WorkspaceCheckRunner::new(WorkspaceCheckRunnerOptions::new(rules, None, SCHEME), cache);
	let expected = cold
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("cold evaluation after suppression change");
	assert_eq!(cached.evaluation.mode, WorkspaceEvaluationMode::Full);
	assert_eq!(cached.diagnostics, expected.diagnostics);
}

#[test]
fn hot_index_recomputes_line_statistics_for_the_affected_group() {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	for container in ["SalesA", "SalesB", "SalesC"] {
		write(
			fixture.path(),
			&format!("src/main/java/com/acme/sales/{container}.java"),
			&format!("package com.acme.sales;\n\nclass {container} {{\n\tclass Invoice {{}}\n}}\n"),
		);
	}
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[workspace.group.where]]
id = "balanced-invoice-size"
severity = "warn"
members = "name = 'Invoice'"
group_by = ["lang", "segment('package')"]
expr = "count(member) >= 3 => avg(member, lines) <= 2"
message = "Invoice size distribution in {group}: {observations}"
"#,
	)
	.expect("statistic rules");
	let cache = LocalResourceCache::default();
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![fixture.path().to_path_buf()], None),
		cache.clone(),
	);
	let transition = registry
		.commands()
		.refresh(WorkspaceRequest::new("workspace-group-statistic-seed"));
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let initial = registry.queries().snapshot().expect("initial snapshot");
	let mut runner = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(rules.clone(), None, SCHEME),
		cache.clone(),
	);
	let seeded = runner
		.run_check(&initial.index, &initial.linkage)
		.expect("seeded statistic evaluation");
	assert!(
		seeded
			.diagnostics
			.iter()
			.all(|diagnostic| diagnostic.rule_id != STATISTIC_RULE_ID)
	);

	let changed_path = fixture
		.path()
		.join("src/main/java/com/acme/sales/SalesC.java");
	write(
		fixture.path(),
		"src/main/java/com/acme/sales/SalesC.java",
		"package com.acme.sales;\n\nclass SalesC {\n\tclass Invoice {\n\t\tint first;\n\t\tint second;\n\t\tint third;\n\t\tint fourth;\n\t}\n}\n",
	);
	let transition = registry.commands().refresh_paths(
		WorkspaceRequest::new("workspace-group-statistic-change"),
		vec![changed_path],
	);
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let changed = registry.queries().snapshot().expect("changed snapshot");
	let incremental = runner
		.run_check(&changed.index, &changed.linkage)
		.expect("incremental statistic evaluation");
	let mut cold_runner =
		WorkspaceCheckRunner::new(WorkspaceCheckRunnerOptions::new(rules, None, SCHEME), cache);
	let cold = cold_runner
		.run_check(&changed.index, &changed.linkage)
		.expect("cold statistic evaluation");

	assert_eq!(
		incremental.evaluation.mode,
		WorkspaceEvaluationMode::Incremental
	);
	assert_eq!(incremental.evaluation.affected_groups, 1);
	assert_eq!(incremental.diagnostics, cold.diagnostics);
	let statistic = incremental
		.diagnostics
		.iter()
		.find(|diagnostic| diagnostic.rule_id == STATISTIC_RULE_ID)
		.expect("statistic violation");
	assert!(statistic.message.contains("avg(member, lines)="));
	assert!(statistic.message.contains("3/3 line ranges"));
}
