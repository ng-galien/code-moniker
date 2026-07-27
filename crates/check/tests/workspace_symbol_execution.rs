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
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![fixture.path().to_path_buf()], None),
		cache.clone(),
	);
	let transition = registry
		.commands()
		.refresh(WorkspaceRequest::new("workspace-symbol-runner"));
	assert!(
		matches!(transition, WorkspaceTransition::Ready { .. }),
		"fixture indexing failed: {:?}",
		registry.queries().last_failure()
	);
	let snapshot = registry.queries().snapshot().expect("snapshot");
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
	let mut runner =
		WorkspaceCheckRunner::new(WorkspaceCheckRunnerOptions::new(rules, None, SCHEME), cache);
	let diagnostics = runner
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("snapshot check");
	let workspace_diagnostics = diagnostics
		.diagnostics
		.iter()
		.filter(|diagnostic| diagnostic.rule_id == RULE_ID)
		.collect::<Vec<_>>();
	assert_eq!(workspace_diagnostics.len(), 1);
	let bad_symbol = workspace_diagnostics[0]
		.symbol
		.expect("workspace diagnostic symbol");
	assert_eq!(
		snapshot
			.index
			.symbols
			.file_records(bad_symbol.file())
			.get(bad_symbol.def())
			.expect("diagnostic symbol")
			.name,
		"BadRepository"
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
