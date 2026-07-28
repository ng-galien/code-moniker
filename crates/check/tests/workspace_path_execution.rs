use std::fs;
use std::path::Path;

use code_moniker_check::workspace::{WorkspaceCheckRunner, WorkspaceCheckRunnerOptions};
use code_moniker_check::{CheckRequest, DefaultRulesSelection, RuleSetRequest, RuleVerdict};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceTransition};
use code_moniker_workspace::source::LocalResourceCache;

const SCHEME: &str = "code+moniker://";
const FORBIDDEN: &str = "workspace.path.danger-must-not-reach-sink";
const REQUIRED: &str = "workspace.path.entry-must-reach-audit";
const BOUNDED: &str = "workspace.path.danger-reaches-sink-with-one-hop";
const INITIAL_SOURCE: &str = r#"
pub fn danger() { middle(); }
fn middle() { sink(); }
pub fn sink() {}

pub fn entry() { helper(); }
fn helper() { audit(); }
pub fn audit() {}
"#;
const CHANGED_SOURCE: &str = r#"
pub fn danger() {}
pub fn sink() {}
pub fn entry() {}
pub fn audit() {}
"#;
const PATH_RULES: &str = r#"
default_rules = false

[[workspace.path]]
id = "danger-must-not-reach-sink"
severity = "warn"
from = "shape = 'callable' AND name =~ ^danger"
to = "shape = 'callable' AND name =~ ^sink"
expect = "no_path"
relation = ["calls"]
max_depth = 4
max_symbols = 100
max_edges = 100
max_pairs = 10
min_coverage = 100
message = "Forbidden call chain: {path}"

[[workspace.path]]
id = "entry-must-reach-audit"
severity = "warn"
from = "shape = 'callable' AND name =~ ^entry"
to = "shape = 'callable' AND name =~ ^audit"
expect = "reachable"
relation = ["calls"]
max_depth = 4
max_symbols = 100
max_edges = 100
max_pairs = 10
min_coverage = 100

[[workspace.path]]
id = "danger-reaches-sink-with-one-hop"
severity = "warn"
from = "shape = 'callable' AND name =~ ^danger"
to = "shape = 'callable' AND name =~ ^sink"
expect = "reachable"
relation = ["calls"]
max_depth = 1
max_symbols = 100
max_edges = 100
max_pairs = 10
min_coverage = 100
"#;

fn write(root: &Path, path: &str, source: &str) {
	let target = root.join(path);
	fs::create_dir_all(target.parent().expect("fixture parent")).expect("fixture directory");
	fs::write(target, source).expect("fixture source");
}

fn report<'a>(
	diagnostics: &'a code_moniker_check::workspace::WorkspaceRuleDiagnostics,
	rule_id: &str,
) -> &'a code_moniker_check::RuleReport {
	diagnostics
		.reports
		.iter()
		.find(|report| report.rule_id == rule_id)
		.expect("path report")
}

fn assert_linked(diagnostics: &code_moniker_check::workspace::WorkspaceRuleDiagnostics) {
	assert!(
		diagnostics
			.diagnostics
			.iter()
			.any(|diagnostic| diagnostic.rule_id == FORBIDDEN)
	);
	assert!(
		diagnostics
			.diagnostics
			.iter()
			.all(|diagnostic| diagnostic.rule_id != REQUIRED)
	);
	let forbidden = report(diagnostics, FORBIDDEN);
	assert_eq!(forbidden.verdict, Some(RuleVerdict::Fail));
	let forbidden_path = forbidden.path.as_ref().expect("forbidden path details");
	assert_eq!(forbidden_path.witness.len(), 2);
	assert_eq!(forbidden_path.max_depth, 4);
	assert_eq!(forbidden_path.max_pairs, 10);
	assert_eq!(
		forbidden.coverage.as_ref().map(|coverage| coverage.percent),
		Some(100)
	);
	let required = report(diagnostics, REQUIRED);
	assert_eq!(required.verdict, Some(RuleVerdict::Pass));
	assert_eq!(
		required.path.as_ref().map(|path| path.witness.len()),
		Some(2)
	);
	let bounded = report(diagnostics, BOUNDED);
	assert_eq!(bounded.verdict, Some(RuleVerdict::Inconclusive));
	let bounded_path = bounded.path.as_ref().expect("bounded path details");
	assert!(bounded_path.depth_limit_reached);
	assert!(
		bounded_path
			.reasons
			.iter()
			.any(|reason| reason == "depth_limit")
	);
}

fn assert_unlinked(diagnostics: &code_moniker_check::workspace::WorkspaceRuleDiagnostics) {
	assert!(
		diagnostics
			.diagnostics
			.iter()
			.all(|diagnostic| diagnostic.rule_id != FORBIDDEN)
	);
	assert!(
		diagnostics
			.diagnostics
			.iter()
			.any(|diagnostic| diagnostic.rule_id == REQUIRED)
	);
	assert_eq!(
		report(diagnostics, FORBIDDEN).verdict,
		Some(RuleVerdict::Pass)
	);
	assert_eq!(
		report(diagnostics, REQUIRED).verdict,
		Some(RuleVerdict::Fail)
	);
	assert_eq!(
		report(diagnostics, BOUNDED).verdict,
		Some(RuleVerdict::Fail)
	);
}

#[test]
fn forbidden_and_required_paths_follow_the_bounded_linkage_engine() {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	let source = fixture.path().join("src/lib.rs");
	write(fixture.path(), "src/lib.rs", INITIAL_SOURCE);
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(&rules, PATH_RULES).expect("rules");
	let cache = LocalResourceCache::default();
	let mut registry = LocalWorkspaceRegistry::local_with_cache(
		LocalWorkspaceOptions::new(vec![fixture.path().to_path_buf()], None),
		cache.clone(),
	);
	assert!(matches!(
		registry
			.commands()
			.refresh(WorkspaceRequest::new("workspace-path-seed")),
		WorkspaceTransition::Ready { .. }
	));
	let initial = registry.queries().snapshot().expect("initial snapshot");
	let mut runner =
		WorkspaceCheckRunner::new(WorkspaceCheckRunnerOptions::new(rules, None, SCHEME), cache);

	let first = runner
		.run_check(&initial.index, &initial.linkage)
		.expect("initial path check");
	assert_linked(&first);

	fs::write(&source, CHANGED_SOURCE).expect("remove both paths");
	assert!(matches!(
		registry
			.commands()
			.refresh_paths(WorkspaceRequest::new("workspace-path-remove"), vec![source],),
		WorkspaceTransition::Ready { .. }
	));
	let changed = registry.queries().snapshot().expect("changed snapshot");
	let second = runner
		.run_check(&changed.index, &changed.linkage)
		.expect("changed path check");
	assert_unlinked(&second);
}

#[test]
fn one_shot_check_builds_linkage_for_workspace_path_rules() {
	const CUSTOM_SCHEME: &str = "custom+moniker://";
	let fixture = tempfile::tempdir().expect("workspace fixture");
	write(fixture.path(), "src/lib.rs", INITIAL_SOURCE);
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(&rules, PATH_RULES).expect("rules");

	let run = CheckRequest::new(
		fixture.path(),
		RuleSetRequest::with_rules(&rules, CUSTOM_SCHEME)
			.with_default_rules(DefaultRulesSelection::Disabled),
	)
	.with_report(true)
	.run()
	.expect("one-shot workspace path check");

	assert!(run.errors.is_empty(), "{:?}", run.errors);
	let violation = run
		.file_violations()
		.find(|(_, violation)| violation.rule_id == FORBIDDEN)
		.expect("forbidden path violation");
	assert!(
		violation.1.moniker.starts_with(CUSTOM_SCHEME),
		"{}",
		violation.1.moniker
	);
	let report = run
		.reports
		.iter()
		.flat_map(|file| &file.rule_reports)
		.find(|report| report.rule_id == FORBIDDEN)
		.expect("one-shot path report");
	assert_eq!(report.verdict, Some(RuleVerdict::Fail));
	assert_eq!(report.path.as_ref().map(|path| path.witness.len()), Some(2));

	let file_scoped = CheckRequest::new(
		fixture.path(),
		RuleSetRequest::with_rules(&rules, CUSTOM_SCHEME)
			.with_default_rules(DefaultRulesSelection::Disabled),
	)
	.with_files(vec!["src/lib.rs".into()])
	.run()
	.expect("file-scoped path check");
	assert!(
		file_scoped
			.errors
			.iter()
			.any(|error| error.error.contains("workspace rules were not run")),
		"{:#?}",
		file_scoped.errors
	);
}

#[test]
fn excluded_sources_cannot_bridge_a_workspace_path() {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	write(
		fixture.path(),
		"src/lib.rs",
		"mod generated;\nmod target;\npub fn entry() { generated::bridge(); }\n",
	);
	write(
		fixture.path(),
		"src/generated.rs",
		"pub fn bridge() { crate::target::sink(); }\n",
	);
	write(fixture.path(), "src/target.rs", "pub fn sink() {}\n");
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[exclude]
uris = ["**/generated.rs"]

[[workspace.path]]
id = "entry-must-not-reach-sink"
severity = "warn"
from = "shape = 'callable' AND name =~ ^entry"
to = "shape = 'callable' AND name =~ ^sink"
expect = "no_path"
relation = ["calls"]
max_depth = 4
max_symbols = 100
max_edges = 100
max_pairs = 10
min_coverage = 100
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
			.refresh(WorkspaceRequest::new("workspace-path-excluded-bridge")),
		WorkspaceTransition::Ready { .. }
	));
	let snapshot = registry.queries().snapshot().expect("snapshot");
	let mut runner =
		WorkspaceCheckRunner::new(WorkspaceCheckRunnerOptions::new(rules, None, SCHEME), cache);

	let diagnostics = runner
		.run_check(&snapshot.index, &snapshot.linkage)
		.expect("path check");
	let report = report(&diagnostics, "workspace.path.entry-must-not-reach-sink");
	assert_eq!(report.verdict, Some(RuleVerdict::Pass), "{report:?}");
	assert!(
		diagnostics
			.diagnostics
			.iter()
			.all(|diagnostic| diagnostic.rule_id != "workspace.path.entry-must-not-reach-sink")
	);
}

#[test]
fn aggregate_symbol_budget_is_exposed_as_a_limit_flag() {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	write(
		fixture.path(),
		"src/lib.rs",
		"pub fn source_one() {}\npub fn source_two() {}\npub fn sink() {}\n",
	);
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(
		&rules,
		r#"
default_rules = false

[[workspace.path]]
id = "entry-reaches-sink"
severity = "warn"
from = "shape = 'callable' AND name =~ ^source_"
to = "shape = 'callable' AND name =~ ^sink"
expect = "reachable"
relation = ["calls"]
max_depth = 4
max_symbols = 1
max_edges = 100
max_pairs = 10
min_coverage = 100
"#,
	)
	.expect("rules");

	let run = CheckRequest::new(
		fixture.path(),
		RuleSetRequest::with_rules(&rules, SCHEME)
			.with_default_rules(DefaultRulesSelection::Disabled),
	)
	.with_report(true)
	.run()
	.expect("budgeted path check");
	let report = run
		.reports
		.iter()
		.flat_map(|file| &file.rule_reports)
		.find(|report| report.rule_id == "workspace.path.entry-reaches-sink")
		.expect("path report");
	assert_eq!(report.verdict, Some(RuleVerdict::Inconclusive));
	let path = report.path.as_ref().expect("path details");
	assert!(path.symbol_limit_reached, "{path:?}");
	assert!(
		path.reasons.iter().any(|reason| reason == "symbol_limit"),
		"{path:?}"
	);
}

#[test]
fn one_shot_suppression_realigns_workspace_path_report() {
	let fixture = tempfile::tempdir().expect("workspace fixture");
	write(
		fixture.path(),
		"src/lib.rs",
		&format!("// code-moniker: ignore-file[danger-must-not-reach-sink]\n{INITIAL_SOURCE}"),
	);
	let rules = fixture.path().join(".code-moniker.toml");
	fs::write(&rules, PATH_RULES).expect("rules");

	let run = CheckRequest::new(
		fixture.path(),
		RuleSetRequest::with_rules(&rules, SCHEME)
			.with_default_rules(DefaultRulesSelection::Disabled),
	)
	.with_report(true)
	.run()
	.expect("suppressed one-shot path check");
	assert!(
		run.file_violations()
			.all(|(_, violation)| violation.rule_id != FORBIDDEN)
	);
	let report = run
		.reports
		.iter()
		.flat_map(|file| &file.rule_reports)
		.find(|report| report.rule_id == FORBIDDEN)
		.expect("path report");
	assert_eq!(report.violations, 0);
	assert_eq!(report.verdict, Some(RuleVerdict::Pass));
}
