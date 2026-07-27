use std::fs;
use std::path::Path;

use code_moniker_check::workspace::{WorkspaceCheckRunner, WorkspaceCheckRunnerOptions};
use code_moniker_check::{CheckRequest, DefaultRulesSelection, RuleSetRequest};
use code_moniker_workspace::registry::{LocalWorkspaceOptions, LocalWorkspaceRegistry};
use code_moniker_workspace::snapshot::{WorkspaceRequest, WorkspaceTransition};
use code_moniker_workspace::source::LocalResourceCache;

const SCHEME: &str = "code+moniker://";
const RULE_ID: &str = "workspace.group.unique-type-name-per-package";

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
	let mut runner =
		WorkspaceCheckRunner::new(WorkspaceCheckRunnerOptions::new(rules, None, SCHEME), cache);
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
}
