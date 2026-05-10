#![cfg(feature = "cli")]
//! End-to-end CLI tests. Each test writes a fixture file via `tempfile`,
//! drives `cli::run` directly with captured writers (no subprocess), and
//! asserts on stdout/stderr/exit. Black-box on the public CLI surface.

use std::io::Write;

use clap::Parser;
use pg_code_moniker::cli::{self, Args, Exit};

const TS_FIXTURE: &str = r#"// header comment
export class Foo {
    bar(s: string): void {
        // inside method
    }
    baz(): void {}
}

class Bar extends Foo {}
"#;

fn run_with(argv: Vec<&str>) -> (Exit, String, String) {
	let args = Args::try_parse_from(argv).expect("parse argv");
	let mut out = Vec::new();
	let mut err = Vec::new();
	let exit = cli::run(&args, &mut out, &mut err);
	(
		exit,
		String::from_utf8(out).unwrap(),
		String::from_utf8(err).unwrap(),
	)
}

fn write_fixture(name: &str, body: &str) -> tempfile::TempDir {
	let dir = tempfile::tempdir().unwrap();
	let p = dir.path().join(name);
	let mut f = std::fs::File::create(&p).unwrap();
	f.write_all(body.as_bytes()).unwrap();
	dir
}

#[test]
fn no_predicate_dumps_full_graph_as_tsv() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec!["pg-moniker", path.to_str().unwrap()]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.lines().any(|l| l.starts_with("def\t")), "{out}");
	assert!(out.contains("class:Foo"), "{out}");
	assert!(out.contains("class:Bar"), "{out}");
}

#[test]
fn count_only_prints_an_integer() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec!["pg-moniker", path.to_str().unwrap(), "--count"]);
	assert_eq!(exit, Exit::Match);
	let trimmed = out.trim();
	let n: usize = trimmed.parse().expect("expected integer, got {trimmed}");
	assert!(n > 0);
}

#[test]
fn quiet_emits_nothing_on_match() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"pg-moniker",
		path.to_str().unwrap(),
		"--kind",
		"comment",
		"--quiet",
	]);
	assert_eq!(exit, Exit::Match);
	assert!(out.is_empty(), "expected silent stdout, got {out}");
}

#[test]
fn no_match_returns_exit_one() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, _, _) = run_with(vec![
		"pg-moniker",
		path.to_str().unwrap(),
		"--kind",
		"does_not_exist",
		"--quiet",
	]);
	assert_eq!(exit, Exit::NoMatch);
}

#[test]
fn descendant_of_filters_to_class_members() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"pg-moniker",
		path.to_str().unwrap(),
		"--descendant-of",
		"ts+moniker://./lang:ts/module:single-file/class:Foo",
		"--kind",
		"method",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err} stdout={out}");
	let lines: Vec<&str> = out.lines().collect();
	assert!(!lines.is_empty(), "no methods matched");
	for line in &lines {
		assert!(line.contains("class:Foo"), "non-Foo descendant: {line}");
		assert!(
			line.contains("method") && line.starts_with("def\t"),
			"unexpected: {line}"
		);
	}
}

#[test]
fn json_format_produces_parsable_document() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"pg-moniker",
		path.to_str().unwrap(),
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::Match);
	let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
	assert_eq!(v["lang"].as_str(), Some("ts"));
	assert!(v["matches"]["defs"].as_array().unwrap().len() > 1);
}

#[test]
fn comment_kind_filter_finds_comments() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"pg-moniker",
		path.to_str().unwrap(),
		"--kind",
		"comment",
		"--count",
	]);
	assert_eq!(exit, Exit::Match);
	let n: usize = out.trim().parse().unwrap();
	assert_eq!(n, 2, "expected two comments, got {n}");
}

#[test]
fn with_text_attaches_comment_source() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"pg-moniker",
		path.to_str().unwrap(),
		"--kind",
		"comment",
		"--with-text",
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::Match);
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	let texts: Vec<&str> = v["matches"]["defs"]
		.as_array()
		.unwrap()
		.iter()
		.filter_map(|d| d["text"].as_str())
		.collect();
	assert!(
		texts.iter().any(|t| t.contains("header comment")),
		"missing top-level comment text: {texts:?}"
	);
	assert!(
		texts.iter().any(|t| t.contains("inside method")),
		"missing nested comment text: {texts:?}"
	);
}

#[test]
fn unknown_extension_is_usage_error() {
	let dir = write_fixture("a.txt", "hello");
	let path = dir.path().join("a.txt");
	let (exit, _, err) = run_with(vec!["pg-moniker", path.to_str().unwrap()]);
	assert_eq!(exit, Exit::UsageError);
	assert!(err.contains("unsupported"), "{err}");
}

#[test]
fn malformed_predicate_uri_is_usage_error() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, _, err) = run_with(vec![
		"pg-moniker",
		path.to_str().unwrap(),
		"--eq",
		"not a uri",
	]);
	assert_eq!(exit, Exit::UsageError);
	assert!(err.contains("--eq"), "{err}");
}
