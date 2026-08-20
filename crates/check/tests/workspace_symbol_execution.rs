use std::fs;
use std::path::Path;

use code_moniker_check::workspace::{
	WorkspaceCheckRunner, WorkspaceCheckRunnerOptions, WorkspaceEvaluationMode,
	WorkspaceRuleDiagnostics,
};
use code_moniker_check::{CheckRequest, DefaultRulesSelection, RuleSetRequest};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceTransition};
use code_moniker_workspace::source::LocalResourceCache;

const SCHEME: &str = "code+moniker://";
const RULE_ID: &str = "workspace.symbol.repositories-under-infra";

fn write(root: &Path, path: &str, source: &str) {
	let target = root.join(path);
	fs::create_dir_all(target.parent().expect("fixture parent")).expect("fixture directory");
	fs::write(target, source).expect("fixture source");
}

fn workspace_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	write(
		fixture.path(),
		"src/main/java/com/acme/infra/GoodRepository.java",
		"package com.acme.infra;\n\npublic class GoodRepository {}\n",
	);
	write(
		fixture.path(),
		"src/main/java/com/acme/domain/BadRepository.java",
		"package com.acme.domain;\n\npublic class BadRepository {}\n",
	);
	write(
		fixture.path(),
		"src/main/java/com/acme/domain/SuppressedRepository.java",
		"package com.acme.domain;\n\n// code-moniker: ignore[workspace.symbol.repositories-under-infra]\npublic class SuppressedRepository {}\n",
	);
	write(
		fixture.path(),
		"generated/java/com/acme/domain/IgnoredRepository.java",
		"package com.acme.domain;\n\npublic class IgnoredRepository {}\n",
	);
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[exclude]
uris = ["**/generated/**"]

[[workspace.symbol.where]]
id = "repositories-under-infra"
severity = "warn"
expr = "(shape = 'type' AND name =~ Repository$) => (uri ~ '**/dir:infra/**' OR uri ~ '**/package:infra/**')"
"#,
	)
	.expect("rules");
	(fixture, rules)
}

fn indexed_workspace(
	root: &Path,
	rules: &Path,
	label: &str,
	cache: LocalResourceCache,
) -> (
	std::sync::Arc<code_moniker_workspace::snapshot::WorkspaceSnapshot>,
	WorkspaceCheckRunner,
) {
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![root.to_path_buf()], None),
		cache.clone(),
	);
	let transition = registry.commands().refresh(WorkspaceRequest::new(label));
	assert!(
		matches!(transition, WorkspaceTransition::Ready { .. }),
		"fixture indexing failed: {:?}",
		registry.queries().last_failure()
	);
	let snapshot = registry.queries().snapshot_arc().expect("snapshot");
	let runner = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(rules.to_path_buf(), None, SCHEME),
		cache,
	);
	(snapshot, runner)
}

fn flagged_symbol_names(
	snapshot: &code_moniker_workspace::snapshot::WorkspaceSnapshot,
	diagnostics: &WorkspaceRuleDiagnostics,
	rule_id: &str,
) -> Vec<String> {
	diagnostics
		.diagnostics
		.iter()
		.filter(|diagnostic| diagnostic.rule_id == rule_id)
		.map(|diagnostic| {
			let symbol = diagnostic.symbol.expect("workspace diagnostic symbol");
			snapshot
				.index
				.symbols
				.file_records(symbol.file())
				.get(symbol.def())
				.expect("diagnostic symbol")
				.name
				.to_string()
		})
		.collect()
}

#[test]
fn one_shot_and_snapshot_runner_agree_on_workspace_symbol_rule() {
	let (fixture, rules) = workspace_fixture();
	let specs = RuleSetRequest::with_rules(&rules, SCHEME)
		.with_default_rules(DefaultRulesSelection::Disabled)
		.compiled_specs_for_langs(std::iter::empty())
		.expect("workspace rule specs");
	assert_eq!(specs.len(), 1);
	assert_eq!(specs[0].root, "workspace");
	assert_eq!(specs[0].subject, "symbol");
	assert_eq!(specs[0].plan, "t1_inventory");
	assert_eq!(
		specs[0].capabilities,
		vec![
			"name.regex".to_string(),
			"shape.exact".to_string(),
			"uri.path".to_string(),
		]
	);

	let one_shot = CheckRequest::new(
		fixture.path(),
		RuleSetRequest::with_rules(&rules, SCHEME)
			.with_default_rules(DefaultRulesSelection::Disabled),
	)
	.run()
	.expect("one-shot check");
	let one_shot_violations = one_shot.file_violations().collect::<Vec<_>>();
	assert_eq!(one_shot_violations.len(), 1);
	assert_eq!(one_shot_violations[0].1.rule_id, RULE_ID);
	assert!(one_shot_violations[0].0.ends_with("BadRepository.java"));
	let file_scoped = CheckRequest::new(
		fixture.path(),
		RuleSetRequest::with_rules(&rules, SCHEME)
			.with_default_rules(DefaultRulesSelection::Disabled),
	)
	.with_files(vec![
		"src/main/java/com/acme/domain/BadRepository.java".into(),
	])
	.run()
	.expect("file-scoped check");
	assert!(
		file_scoped
			.errors
			.iter()
			.any(|error| error.error.contains("workspace rules were not run")),
		"{:#?}",
		file_scoped.errors
	);

	let cache = LocalResourceCache::default();
	let (snapshot, mut runner) =
		indexed_workspace(fixture.path(), &rules, "workspace-symbol-runner", cache);
	assert!(
		snapshot
			.index
			.inventory
			.all_symbols()
			.iter()
			.filter_map(|ordinal| snapshot.index.inventory.record(ordinal))
			.any(|record| record.name.as_ref() == "IgnoredRepository"),
		"the daemon inventory must contain excluded files so the check runner owns exclusion"
	);
	let diagnostics = runner
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("snapshot check");
	assert_eq!(
		flagged_symbol_names(&snapshot, &diagnostics, RULE_ID),
		vec!["BadRepository".to_string()]
	);
}

#[test]
fn incremental_runner_keeps_excluded_symbols_out_of_rule_universe() {
	let (fixture, rules) = workspace_fixture();
	let cache = LocalResourceCache::default();
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![fixture.path().to_path_buf()], None),
		cache.clone(),
	);
	let transition = registry
		.commands()
		.refresh(WorkspaceRequest::new("workspace-symbol-excluded-seed"));
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let initial = registry.queries().snapshot().expect("initial snapshot");
	let mut runner = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(rules.clone(), None, SCHEME),
		cache.clone(),
	);
	runner
		.run_check(&initial.index, &initial.linkage)
		.expect("seed workspace symbol evaluation");

	let excluded = fixture
		.path()
		.join("generated/java/com/acme/domain/IgnoredRepository.java");
	write(
		fixture.path(),
		"generated/java/com/acme/domain/IgnoredRepository.java",
		"package com.acme.domain;\n\npublic class ChangedIgnoredRepository {}\n",
	);
	let transition = registry.commands().refresh_paths(
		WorkspaceRequest::new("workspace-symbol-excluded-refresh"),
		vec![excluded],
	);
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let changed = registry.queries().snapshot().expect("changed snapshot");
	let incremental = runner
		.run_check(&changed.index, &changed.linkage)
		.expect("incremental excluded evaluation");
	let mut cold =
		WorkspaceCheckRunner::new(WorkspaceCheckRunnerOptions::new(rules, None, SCHEME), cache);
	let expected = cold
		.run_check(&changed.index, &changed.linkage)
		.expect("cold excluded evaluation");
	assert_eq!(
		incremental.evaluation.mode,
		WorkspaceEvaluationMode::Incremental
	);
	assert_eq!(incremental.diagnostics, expected.diagnostics);
}

#[test]
fn disjoint_workspace_symbol_rule_stays_on_the_inventory_bitmaps() {
	const DISJOINT_RULE_ID: &str = "workspace.symbol.repositories-outside-domain";
	let fixture = tempfile::tempdir().expect("workspace fixture");
	write(
		fixture.path(),
		"src/main/java/com/acme/infra/GoodRepository.java",
		"package com.acme.infra;\n\npublic class GoodRepository {}\n",
	);
	write(
		fixture.path(),
		"src/main/java/com/acme/domain/Order.java",
		"package com.acme.domain;\n\npublic class Order {}\n",
	);
	write(
		fixture.path(),
		"src/main/java/com/acme/domain/BadRepository.java",
		"package com.acme.domain;\n\npublic class BadRepository {}\n",
	);
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[workspace.symbol.where]]
id = "repositories-outside-domain"
severity = "warn"
expr = "(shape = 'type' AND name =~ Repository$) disjoint uri ~ '**/package:domain/**'"
"#,
	)
	.expect("rules");

	let specs = RuleSetRequest::with_rules(&rules, SCHEME)
		.with_default_rules(DefaultRulesSelection::Disabled)
		.compiled_specs_for_langs(std::iter::empty())
		.expect("workspace rule specs");
	assert_eq!(specs.len(), 1);
	assert_eq!(
		specs[0].plan, "t1_inventory",
		"`disjoint` desugars to NOT/AND, so the rule keeps the bitmap-backed inventory plan"
	);

	let (snapshot, mut runner) = indexed_workspace(
		fixture.path(),
		&rules,
		"workspace-disjoint",
		LocalResourceCache::default(),
	);
	let diagnostics = runner
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("snapshot check");
	assert_eq!(
		flagged_symbol_names(&snapshot, &diagnostics, DISJOINT_RULE_ID),
		vec!["BadRepository".to_string()],
		"only the symbol matching both operands violates"
	);
}
