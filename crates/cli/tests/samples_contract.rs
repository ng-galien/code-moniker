//! Contract harness for the executable samples under `samples/`. Every
//! scenario document must replay exactly to its `cm:expect` block, every
//! configured rule must fire at least once, and a sample that demonstrates
//! nothing is rejected. `CM_SCENARIO_BLESS=1` rewrites the expect blocks from
//! the observed violations instead of asserting.

use std::path::{Path, PathBuf};

use code_moniker_check::scenario::Scenario;

const SCHEME: &str = "code+moniker://";

fn samples_dir() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples")
}

fn bless_requested() -> bool {
	std::env::var_os("CM_SCENARIO_BLESS").is_some_and(|value| value == "1")
}

#[test]
fn samples_match_their_expectations() {
	let mut checked = 0;
	for entry in std::fs::read_dir(samples_dir()).expect("samples directory") {
		let path = entry.expect("samples entry").path();
		if path.extension().is_none_or(|extension| extension != "md")
			|| path.file_name().is_some_and(|name| name == "README.md")
		{
			continue;
		}
		check_sample(&path);
		checked += 1;
	}
	assert!(
		checked >= 2,
		"expected scenario samples in {}",
		samples_dir().display()
	);
}

fn check_sample(path: &Path) {
	let document = std::fs::read_to_string(path).expect("read sample");
	let scenario =
		Scenario::parse(&document).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
	let temp = tempfile::tempdir().expect("tempdir");
	scenario.materialize(temp.path()).expect("materialize");
	let run = scenario.run(temp.path(), SCHEME).expect("run scenario");
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
		"{}: rules never fired: {}",
		path.display(),
		run.silent_rules.join(", ")
	);
	assert!(
		!run.actual.is_empty(),
		"{} demonstrates no violation",
		path.display()
	);
}
