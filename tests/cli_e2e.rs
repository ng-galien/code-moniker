#![cfg(feature = "cli")]
//! End-to-end CLI tests. Each test writes a fixture file via `tempfile`,
//! drives `cli::run` directly with captured writers (no subprocess), and
//! asserts on stdout/stderr/exit. Black-box on the public CLI surface.

use std::io::Write;

use clap::Parser;
use code_moniker::cli::{self, Cli, Exit};

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
	let cli = Cli::try_parse_from(argv).expect("parse argv");
	let mut out = Vec::new();
	let mut err = Vec::new();
	let exit = cli::run(&cli, &mut out, &mut err);
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
	let (exit, out, err) = run_with(vec!["code-moniker", path.to_str().unwrap()]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.lines().any(|l| l.starts_with("def\t")), "{out}");
	assert!(out.contains("class:Foo"), "{out}");
	assert!(out.contains("class:Bar"), "{out}");
}

#[test]
fn count_only_prints_an_integer() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec!["code-moniker", path.to_str().unwrap(), "--count"]);
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
		"code-moniker",
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
		"code-moniker",
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
		"code-moniker",
		path.to_str().unwrap(),
		"--where",
		"<@ ts+moniker://./lang:ts/module:single-file/class:Foo",
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
		"code-moniker",
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
		"code-moniker",
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
		"code-moniker",
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
	let (exit, _, err) = run_with(vec!["code-moniker", path.to_str().unwrap()]);
	assert_eq!(exit, Exit::UsageError);
	assert!(err.contains("unsupported"), "{err}");
}

#[test]
fn malformed_predicate_uri_is_usage_error() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, _, err) = run_with(vec![
		"code-moniker",
		path.to_str().unwrap(),
		"--where",
		"= not a uri",
	]);
	assert_eq!(exit, Exit::UsageError);
	assert!(err.contains("--where"), "{err}");
}

const TS_BAD_NAMING: &str = "class lower_case_class {}\n";

#[test]
fn check_clean_file_returns_match() {
	let dir = write_fixture("a.ts", "class GoodName {}\n");
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out} stderr={err}");
	assert!(out.is_empty(), "no violations expected: {out}");
}

#[test]
fn check_violation_reports_rule_id_and_lines() {
	let dir = write_fixture("a.ts", TS_BAD_NAMING);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
	]);
	assert_eq!(exit, Exit::NoMatch);
	assert!(out.contains("ts.class.name-pascalcase"), "{out}");
	assert!(out.contains("L1-L1"), "{out}");
}

#[test]
fn check_json_format_is_structured() {
	let dir = write_fixture("a.ts", TS_BAD_NAMING);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::NoMatch);
	let v: serde_json::Value = serde_json::from_str(&out).expect("json output");
	assert_eq!(v["summary"]["files_scanned"], 1);
	assert_eq!(v["summary"]["files_with_violations"], 1);
	let files = v["files"].as_array().unwrap();
	assert_eq!(files.len(), 1);
	assert!(files[0]["file"].as_str().unwrap().ends_with("a.ts"));
	let viols = files[0]["violations"].as_array().unwrap();
	assert_eq!(viols[0]["rule_id"], "ts.class.name-pascalcase");
}

#[test]
fn check_project_walks_directory_and_aggregates() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::write(dir.path().join("good.ts"), "class GoodName {}\n").unwrap();
	std::fs::write(dir.path().join("bad.ts"), TS_BAD_NAMING).unwrap();
	std::fs::write(dir.path().join("README.md"), "not a source file\n").unwrap();
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
	]);
	assert_eq!(exit, Exit::NoMatch, "stdout={out}");
	assert!(out.contains("bad.ts"), "{out}");
	assert!(!out.contains("good.ts"), "{out}");
	assert!(out.contains("ts.class.name-pascalcase"), "{out}");
	assert!(out.contains("violation(s) across 1 file(s)"), "{out}");
}

#[test]
fn check_project_respects_gitignore() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::write(dir.path().join(".gitignore"), "ignored.ts\n").unwrap();
	std::fs::write(dir.path().join("scanned.ts"), TS_BAD_NAMING).unwrap();
	std::fs::write(dir.path().join("ignored.ts"), TS_BAD_NAMING).unwrap();
	// `ignore` only honors .gitignore inside a git repo, so init one.
	std::fs::create_dir(dir.path().join(".git")).unwrap();
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
	]);
	assert_eq!(exit, Exit::NoMatch);
	assert!(out.contains("scanned.ts"), "{out}");
	assert!(!out.contains("ignored.ts"), "{out}");
}

#[test]
fn check_project_clean_returns_match() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::write(dir.path().join("a.ts"), "class GoodName {}\n").unwrap();
	std::fs::write(dir.path().join("b.ts"), "class AlsoGood {}\n").unwrap();
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
	]);
	assert_eq!(exit, Exit::Match);
	assert!(out.contains("0 violation(s)"), "{out}");
}

#[test]
fn check_project_json_has_summary_and_files() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::write(dir.path().join("bad.ts"), TS_BAD_NAMING).unwrap();
	std::fs::write(dir.path().join("good.ts"), "class GoodName {}\n").unwrap();
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::NoMatch);
	let v: serde_json::Value = serde_json::from_str(&out).expect("json output");
	assert_eq!(v["summary"]["files_scanned"], 2);
	assert_eq!(v["summary"]["files_with_violations"], 1);
	assert_eq!(v["summary"]["total_violations"], 1);
}

#[test]
fn check_require_doc_comment_flags_undocumented_public_class() {
	let dir = write_fixture("a.ts", "export class Foo {}\n");
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[ts.class]
		require_doc_comment = "public"
		"#,
	)
	.unwrap();
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::NoMatch);
	assert!(out.contains("ts.class.require_doc_comment"), "{out}");
}

#[test]
fn check_require_doc_comment_passes_when_docblock_precedes() {
	let dir = write_fixture("a.ts", "/** doc */\nexport class Foo {}\n");
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[ts.class]
		require_doc_comment = "public"
		"#,
	)
	.unwrap();
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::Match, "stdout={out}");
}

#[test]
fn check_default_preset_flags_helper_function_name() {
	let dir = write_fixture("a.ts", "function helper() {}\n");
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
	]);
	assert_eq!(exit, Exit::NoMatch);
	assert!(
		out.contains("ts.function.no-placeholder-names"),
		"expected forbid_name_patterns violation: {out}"
	);
}

#[test]
fn check_user_overlay_keeps_default_forbid_when_changing_max_lines() {
	let dir = write_fixture("a.ts", "function helper() {}\n");
	let rules_path = dir.path().join("rules.toml");
	// Override the preset's max-lines rule by reusing its id; the preset's
	// no-placeholder-names rule must survive the override.
	std::fs::write(
		&rules_path,
		r#"
		[[ts.function.where]]
		id   = "max-lines"
		expr = "lines <= 999"
		"#,
	)
	.unwrap();
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::NoMatch);
	assert!(
		out.contains("ts.function.no-placeholder-names"),
		"merge regression — preset forbid_name_patterns lost when max_lines override applied: {out}"
	);
}

#[test]
fn check_unknown_kind_in_user_config_is_a_usage_error() {
	let dir = write_fixture("a.ts", "class GoodName {}\n");
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[[ts.classs.where]]
		expr = "name =~ ^X"
		"#,
	)
	.unwrap();
	let path = dir.path().join("a.ts");
	let (exit, _, err) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::UsageError);
	assert!(err.contains("classs"), "{err}");
}

#[test]
fn check_invalid_regex_in_user_config_is_a_usage_error() {
	let dir = write_fixture("a.ts", "class GoodName {}\n");
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[[ts.class.where]]
		expr = "name =~ [unclosed"
		"#,
	)
	.unwrap();
	let path = dir.path().join("a.ts");
	let (exit, _, err) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::UsageError);
	assert!(
		err.to_lowercase().contains("regex") || err.to_lowercase().contains("invalid"),
		"{err}"
	);
}

#[test]
fn check_explanation_appears_in_text_and_json() {
	let dir = write_fixture("a.ts", TS_BAD_NAMING);
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[[ts.class.where]]
		id      = "name-pascalcase"
		expr    = "name =~ ^[A-Z][A-Za-z0-9]*$"
		message = "Rename `{name}`. See CLAUDE.md §naming."
		"#,
	)
	.unwrap();
	let path = dir.path().join("a.ts");

	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::NoMatch);
	assert!(
		out.contains("  → Rename `lower_case_class`. See CLAUDE.md §naming."),
		"text format missing indented explanation: {out}"
	);

	let (_, out_json, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--format",
		"json",
	]);
	let v: serde_json::Value = serde_json::from_str(&out_json).expect("valid JSON");
	let arr = v["files"][0]["violations"].as_array().unwrap();
	let exp = arr[0]["explanation"]
		.as_str()
		.expect("json carries explanation");
	assert!(exp.contains("CLAUDE.md"), "explanation in JSON: {exp}");
}

#[test]
fn check_user_overlay_relaxes_default_rule() {
	let dir = write_fixture("a.ts", TS_BAD_NAMING);
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[[ts.class.where]]
		id   = "name-pascalcase"
		expr = "name =~ ^[a-zA-Z_][a-zA-Z0-9_]*$"
		"#,
	)
	.unwrap();
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::Match, "user override permits the name: {out}");
}
