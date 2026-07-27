use std::fs;
use std::path::Path;

use code_moniker_check::RuleVerdict;
use code_moniker_check::workspace::{
	WorkspaceCheckRunner, WorkspaceCheckRunnerOptions, WorkspaceEvaluationMode,
};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceTransition};
use code_moniker_workspace::source::LocalResourceCache;

const SCHEME: &str = "code+moniker://";
const RULE_ID: &str = "workspace.symbol.target-has-callers";

fn write(root: &Path, path: &str, source: &str) {
	let target = root.join(path);
	fs::create_dir_all(target.parent().expect("fixture parent")).expect("fixture directory");
	fs::write(target, source).expect("fixture source");
}

#[test]
fn snapshot_runner_evaluates_t2_against_current_linkage() {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	write(fixture.path(), "src/target.rs", "pub fn target() {}\n");
	write(
		fixture.path(),
		"src/lib.rs",
		"mod target;\npub fn build() { target::target(); }\n",
	);
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[workspace]
min_linkage_coverage = 100

[[workspace.symbol.where]]
id = "target-has-callers"
severity = "warn"
expr = "(shape = 'callable' AND name =~ ^target) => count(in_refs) >= 1"
"#,
	)
	.expect("rules");
	let cache = LocalResourceCache::default();
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![fixture.path().to_path_buf()], None),
		cache.clone(),
	);
	let transition = registry
		.commands()
		.refresh(WorkspaceRequest::new("workspace-linkage-seed"));
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let initial = registry.queries().snapshot().expect("initial snapshot");
	assert!(initial.linkage.resolved_refs > 0);
	let mut runner = WorkspaceCheckRunner::new(
		WorkspaceCheckRunnerOptions::new(rules.clone(), None, SCHEME),
		cache,
	);
	let passing = runner
		.run_check(&initial.index, &initial.linkage)
		.expect("passing linkage check");
	let report = passing
		.reports
		.iter()
		.find(|report| report.rule_id == RULE_ID)
		.expect("linkage rule report");
	assert_eq!(report.verdict, Some(RuleVerdict::Pass));
	assert_eq!(
		report.coverage.as_ref().map(|coverage| coverage.percent),
		Some(100)
	);
	assert!(
		passing
			.diagnostics
			.iter()
			.all(|diagnostic| diagnostic.rule_id != RULE_ID)
	);

	let lib = fixture.path().join("src/lib.rs");
	fs::write(&lib, "mod target;\npub fn build() {}\n").expect("remove target reference");
	let transition = registry.commands().refresh_paths(
		WorkspaceRequest::new("workspace-linkage-remove-reference"),
		vec![lib],
	);
	assert!(matches!(transition, WorkspaceTransition::Ready { .. }));
	let changed = registry.queries().snapshot().expect("changed snapshot");
	let failing = runner
		.run_check(&changed.index, &changed.linkage)
		.expect("failing linkage check");
	assert_eq!(failing.evaluation.mode, WorkspaceEvaluationMode::Full);
	assert!(
		failing
			.diagnostics
			.iter()
			.any(|diagnostic| diagnostic.rule_id == RULE_ID)
	);
	assert_eq!(
		failing
			.reports
			.iter()
			.find(|report| report.rule_id == RULE_ID)
			.and_then(|report| report.verdict),
		Some(RuleVerdict::Fail)
	);
}

#[test]
fn excluded_reference_sources_do_not_feed_linkage_metrics() {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	write(fixture.path(), "src/target.rs", "pub fn target() {}\n");
	write(
		fixture.path(),
		"src/generated.rs",
		"pub fn generated() { crate::target::target(); }\n",
	);
	write(
		fixture.path(),
		"src/lib.rs",
		"mod target;\nmod generated;\npub fn build() {}\n",
	);
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[exclude]
uris = ["**/generated.rs"]

[[workspace.symbol.where]]
id = "target-has-callers"
severity = "warn"
expr = "(shape = 'callable' AND name =~ ^target) => count(in_refs) >= 1"
"#,
	)
	.expect("rules");
	let cache = LocalResourceCache::default();
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![fixture.path().to_path_buf()], None),
		cache.clone(),
	);
	assert!(matches!(
		registry
			.commands()
			.refresh(WorkspaceRequest::new("workspace-linkage-excluded-source")),
		WorkspaceTransition::Ready { .. }
	));
	let snapshot = registry.queries().snapshot().expect("snapshot");
	let mut runner =
		WorkspaceCheckRunner::new(WorkspaceCheckRunnerOptions::new(rules, None, SCHEME), cache);

	let diagnostics = runner
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("linkage check");
	assert!(
		diagnostics
			.diagnostics
			.iter()
			.any(|diagnostic| diagnostic.rule_id == RULE_ID),
		"the only caller is excluded from the review surface"
	);
}

#[test]
fn suppression_realigns_linkage_report_verdict() {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	write(
		fixture.path(),
		"src/target.rs",
		"// code-moniker: ignore-file[target-has-callers]\npub fn target() {}\n",
	);
	write(
		fixture.path(),
		"src/lib.rs",
		"mod target;\npub fn build() {}\n",
	);
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[workspace.symbol.where]]
id = "target-has-callers"
severity = "warn"
expr = "(shape = 'callable' AND name =~ ^target) => count(in_refs) >= 1"
"#,
	)
	.expect("rules");
	let cache = LocalResourceCache::default();
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![fixture.path().to_path_buf()], None),
		cache.clone(),
	);
	assert!(matches!(
		registry
			.commands()
			.refresh(WorkspaceRequest::new("workspace-linkage-suppressed")),
		WorkspaceTransition::Ready { .. }
	));
	let snapshot = registry.queries().snapshot().expect("snapshot");
	let mut runner =
		WorkspaceCheckRunner::new(WorkspaceCheckRunnerOptions::new(rules, None, SCHEME), cache);

	let diagnostics = runner
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("linkage check");
	assert!(
		diagnostics
			.diagnostics
			.iter()
			.all(|diagnostic| diagnostic.rule_id != RULE_ID)
	);
	let report = diagnostics
		.reports
		.iter()
		.find(|report| report.rule_id == RULE_ID)
		.expect("linkage report");
	assert_eq!(report.violations, 0);
	assert_eq!(report.verdict, Some(RuleVerdict::Pass));
}
