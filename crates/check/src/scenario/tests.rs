use super::{ExpectedViolation, Scenario};

const DOCUMENT: &str = r#"---
name: rust-naming
lang: rust
blurb: Functions stay snake_case
published: true
---

# Naming

Functions should be snake_case.

```toml cm:rules
[[rust.fn.where]]
id   = "snake-case"
expr = "name =~ ^[a-z][a-z0-9_]*$"
```

```rust cm:file=src/lib.rs
pub fn tidy() {}

pub fn DoThing() {}
```

```cm:expect
rust.fn.snake-case @ src/lib.rs:L3
```
"#;

#[test]
fn parses_front_matter_rules_files_and_expects() {
	let scenario = Scenario::parse(DOCUMENT).expect("parse scenario");
	assert_eq!(scenario.meta.name, "rust-naming");
	assert_eq!(scenario.meta.lang, "rust");
	assert!(scenario.meta.published);
	assert!(
		scenario
			.rules
			.as_deref()
			.expect("rules block")
			.contains("snake-case")
	);
	assert_eq!(scenario.files.len(), 1);
	assert_eq!(scenario.files[0].path, "src/lib.rs");
	assert_eq!(
		scenario.files[0].body,
		"pub fn tidy() {}\n\npub fn DoThing() {}\n"
	);
	assert_eq!(
		scenario.expects,
		vec![ExpectedViolation {
			rule_id: "rust.fn.snake-case".to_string(),
			path: "src/lib.rs".to_string(),
			lines: (3, 3),
		}]
	);
	assert!(!scenario.effective_default_rules());
}

#[test]
fn scenario_runs_against_a_materialized_workspace() {
	let scenario = Scenario::parse(DOCUMENT).expect("parse scenario");
	let temp = tempfile::tempdir().expect("tempdir");
	scenario.materialize(temp.path()).expect("materialize");
	let run = scenario
		.run(temp.path(), "code+moniker://")
		.expect("run scenario");
	assert!(run.is_match(), "mismatch:\n{}", run.mismatch_summary());
	assert_eq!(run.actual.len(), 1);
	assert!(run.silent_rules.is_empty());
}

#[test]
fn mismatched_expectations_report_missing_and_unexpected() {
	let document = DOCUMENT.replace("src/lib.rs:L3", "src/lib.rs:L1");
	let scenario = Scenario::parse(&document).expect("parse scenario");
	let temp = tempfile::tempdir().expect("tempdir");
	scenario.materialize(temp.path()).expect("materialize");
	let run = scenario
		.run(temp.path(), "code+moniker://")
		.expect("run scenario");
	assert!(!run.is_match());
	assert_eq!(run.missing.len(), 1);
	assert_eq!(run.unexpected.len(), 1);
	assert!(run.mismatch_summary().contains("missing:"));
	assert!(run.mismatch_summary().contains("unexpected:"));
}

#[test]
fn bless_rewrites_the_expect_block_in_place() {
	let document = DOCUMENT.replace("src/lib.rs:L3", "src/lib.rs:L1");
	let scenario = Scenario::parse(&document).expect("parse scenario");
	let temp = tempfile::tempdir().expect("tempdir");
	scenario.materialize(temp.path()).expect("materialize");
	let run = scenario
		.run(temp.path(), "code+moniker://")
		.expect("run scenario");
	let blessed = scenario.bless(&document, &run.actual);
	assert_eq!(blessed, DOCUMENT);
}

#[test]
fn bless_appends_an_expect_block_when_missing() {
	let document = DOCUMENT
		.replace(
			"```cm:expect\nrust.fn.snake-case @ src/lib.rs:L3\n```\n",
			"",
		)
		.trim_end()
		.to_string();
	let scenario = Scenario::parse(&document).expect("parse scenario");
	assert!(scenario.expects.is_empty());
	let blessed = scenario.bless(
		&document,
		&[ExpectedViolation {
			rule_id: "rust.fn.snake-case".to_string(),
			path: "src/lib.rs".to_string(),
			lines: (3, 3),
		}],
	);
	assert!(blessed.ends_with("```cm:expect\nrust.fn.snake-case @ src/lib.rs:L3\n```\n"));
	Scenario::parse(&blessed).expect("blessed document still parses");
}

#[test]
fn rejects_escaping_paths_and_duplicates() {
	for path in ["../evil.rs", "/abs.rs", "a/../b.rs", "a//b.rs"] {
		let document = format!("```rust cm:file={path}\nfn x() {{}}\n```\n");
		let error = Scenario::parse(&document).expect_err("path must be rejected");
		assert!(error.message.contains("relative path"), "{error}");
	}
	let duplicated = "```rust cm:file=a.rs\n```\n\n```rust cm:file=a.rs\n```\n";
	let error = Scenario::parse(duplicated).expect_err("duplicate file");
	assert!(error.message.contains("duplicate file"), "{error}");
}

#[test]
fn longer_fences_escape_embedded_backticks() {
	let document = "````md cm:file=docs/note.md\n```\ninner fence\n```\n````\n";
	let scenario = Scenario::parse(document).expect("parse scenario");
	assert_eq!(scenario.files[0].body, "```\ninner fence\n```\n");
}

#[test]
fn unknown_front_matter_keys_and_bad_expects_fail_with_line_numbers() {
	let bad_meta = "---\nnom: x\n---\n";
	let error = Scenario::parse(bad_meta).expect_err("unknown key");
	assert_eq!(error.line, 2);

	let bad_expect = "```cm:expect\nrule-without-location\n```\n";
	let error = Scenario::parse(bad_expect).expect_err("bad expect");
	assert_eq!(error.line, 2);
}
