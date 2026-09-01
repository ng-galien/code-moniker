//! Contract harness for executable samples under `samples/catalog/` and
//! `samples/learn/`. Every scenario document must replay exactly to its
//! `cm:expect` block, every configured rule must be demonstrated by a violation
//! or an exact verdict (or explicitly excused), and a sample that demonstrates
//! nothing is rejected. `CM_SCENARIO_BLESS=1` rewrites the violation entries
//! instead of asserting.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use code_moniker_check::scenario::Scenario;

const SCHEME: &str = "code+moniker://";

fn samples_dirs() -> [PathBuf; 2] {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples");
	[root.join("catalog"), root.join("learn")]
}

fn bless_requested() -> bool {
	std::env::var_os("CM_SCENARIO_BLESS").is_some_and(|value| value == "1")
}

#[test]
fn samples_match_their_expectations() {
	let mut checked = 0;
	for dir in samples_dirs() {
		for entry in std::fs::read_dir(&dir).unwrap_or_else(|error| {
			panic!("samples directory {}: {error}", dir.display());
		}) {
			let path = entry.expect("samples entry").path();
			if !path
				.file_name()
				.and_then(|name| name.to_str())
				.is_some_and(|name| name.ends_with(".cm.md"))
			{
				continue;
			}
			check_sample(&path);
			checked += 1;
		}
	}
	assert!(
		checked >= 2 * samples_dirs().len(),
		"expected executable scenario samples in samples/catalog and samples/learn"
	);
}

#[test]
fn published_catalog_covers_every_workspace_rule_root() {
	let catalog = samples_dirs()[0].clone();
	let documents = std::fs::read_dir(&catalog)
		.expect("catalog directory")
		.filter_map(|entry| {
			let path = entry.ok()?.path();
			path.extension()
				.and_then(|extension| extension.to_str())
				.is_some_and(|extension| extension == "md")
				.then(|| std::fs::read_to_string(path).expect("read catalog sample"))
		})
		.collect::<Vec<_>>();

	for root in [
		"[[workspace.symbol.where]]",
		"[[workspace.group.where]]",
		"[[workspace.path]]",
	] {
		assert!(
			documents
				.iter()
				.any(|document| document.contains("published: true") && document.contains(root)),
			"published catalog does not demonstrate `{root}`"
		);
	}
}

#[test]
fn published_catalog_has_learn_metadata_and_unique_names() {
	let catalog = samples_dirs()[0].clone();
	let mut names = BTreeSet::new();
	for entry in std::fs::read_dir(&catalog).expect("catalog directory") {
		let path = entry.expect("catalog entry").path();
		if !path
			.file_name()
			.and_then(|name| name.to_str())
			.is_some_and(|name| name.ends_with(".cm.md"))
		{
			continue;
		}
		let document = std::fs::read_to_string(&path).expect("read catalog sample");
		let scenario = Scenario::parse(&document)
			.unwrap_or_else(|error| panic!("{}: {error}", path.display()));
		if !scenario.meta.published {
			continue;
		}
		assert!(
			!scenario.meta.name.is_empty(),
			"{}: missing name",
			path.display()
		);
		assert!(
			names.insert(scenario.meta.name.clone()),
			"{}: duplicate catalog name `{}`",
			path.display(),
			scenario.meta.name
		);
		assert!(
			!scenario.meta.title.is_empty(),
			"{}: missing title",
			path.display()
		);
		assert!(
			matches!(
				scenario.meta.learn_kind.as_str(),
				"language" | "framework" | "pattern" | "workspace"
			),
			"{}: invalid learn_kind `{}`",
			path.display(),
			scenario.meta.learn_kind
		);
		assert!(
			!scenario.meta.tags.is_empty(),
			"{}: missing tags",
			path.display()
		);
		assert!(
			!scenario.meta.learn_path.is_empty(),
			"{}: missing learn_path",
			path.display()
		);
	}
}

fn check_sample(path: &Path) {
	let document = std::fs::read_to_string(path).expect("read sample");
	let scenario =
		Scenario::parse(&document).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
	let run = scenario.run(Path::new("."), SCHEME).expect("run scenario");
	if bless_requested() {
		std::fs::write(path, scenario.bless(&document, &run.actual)).expect("bless sample");
		return;
	}
	assert!(
		run.is_match(),
		"{} does not replay to its expectations:\n{}",
		path.display(),
		run.mismatch_summary()
	);
	assert!(
		run.silent_rules.is_empty(),
		"{}: rules lack a violation, exact verdict, or `! <rule-id> <reason>` excuse: {}",
		path.display(),
		run.silent_rules.join(", ")
	);
	assert!(
		run.stale_undemonstrated.is_empty(),
		"{}: rules marked undemonstrated actually fire: {}",
		path.display(),
		run.stale_undemonstrated.join(", ")
	);
	assert!(
		!run.actual.is_empty() || !scenario.verdicts.is_empty(),
		"{} demonstrates neither a violation nor an exact verdict",
		path.display()
	);
}
