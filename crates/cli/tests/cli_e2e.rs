//! End-to-end CLI tests. Each test writes a fixture file via `tempfile`,
//! drives `cli::run` directly with captured writers (no subprocess), and
//! asserts on stdout/stderr/exit. Black-box on the public CLI surface.

use std::io::Write;

use clap::Parser;
use code_moniker_cli::{self as cli, Cli, Exit};

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

fn write_under(root: &std::path::Path, rel: &str, body: &str) {
	let p = root.join(rel);
	if let Some(parent) = p.parent() {
		std::fs::create_dir_all(parent).unwrap();
	}
	let mut f = std::fs::File::create(&p).unwrap();
	f.write_all(body.as_bytes()).unwrap();
}

#[test]
#[cfg(feature = "tui")]
fn ui_help_is_available() {
	let err = Cli::try_parse_from(vec!["code-moniker", "ui", "--help"]).unwrap_err();
	assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
	let help = err.to_string();
	assert!(help.contains("terminal architecture explorer"), "{help}");
	assert!(help.contains("--cache"), "{help}");
	assert!(help.contains("--rules"), "{help}");
}

#[test]
fn no_predicate_dumps_monikers_as_text_by_default() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec!["code-moniker", "extract", path.to_str().unwrap()]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(!out.lines().any(|l| l.starts_with("def\t")), "{out}");
	assert!(out.contains("class:Foo"), "{out}");
	assert!(out.contains("class:Bar"), "{out}");
	assert!(
		!out.contains("code+moniker://"),
		"default text should use compact monikers: {out}"
	);
	for line in out.lines() {
		assert!(
			!line.contains('\t'),
			"text format should contain only monikers: {line}"
		);
	}
}

#[test]
fn extract_tsv_still_emits_table_rows() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--format",
		"tsv",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.lines().any(|l| l.starts_with("def\t")), "{out}");
	assert!(
		out.lines().any(|l| l.matches('\t').count() >= 7),
		"expected TSV metadata columns: {out}"
	);
}

#[test]
fn extract_text_limit_warns_with_next_cursor() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--limit",
		"1",
	]);
	assert_eq!(exit, Exit::Match);
	assert_eq!(out.lines().count(), 1, "{out}");
	assert!(err.contains("more results"), "{err}");
	assert!(err.contains("use --after 'code+moniker://"), "{err}");
	assert!(err.contains("or --all"), "{err}");
}

#[test]
fn extract_json_limit_emits_next_cursor_and_after_resumes() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--format",
		"json",
		"--limit",
		"1",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(
		err.is_empty(),
		"json should carry pagination metadata: {err}"
	);
	let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
	assert_eq!(json_match_count(&v), 1);
	let cursor = v["next_cursor"].as_str().expect("next cursor");
	assert!(cursor.starts_with("code+moniker://"), "{cursor}");
	assert!(v["remaining"].as_u64().is_some_and(|n| n > 0));

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--format",
		"json",
		"--limit",
		"1",
		"--after",
		cursor,
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
	assert_eq!(json_match_count(&v), 1);
	assert_ne!(v["next_cursor"].as_str(), Some(cursor));
}

fn json_match_count(v: &serde_json::Value) -> usize {
	let defs = v["matches"]["defs"].as_array().map_or(0, |defs| defs.len());
	let refs = v["matches"]["refs"].as_array().map_or(0, |refs| refs.len());
	defs + refs
}

#[test]
fn extract_text_accepts_txt_alias() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--format",
		"txt",
		"--kind",
		"class",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.contains("class:Foo"), "{out}");
	assert!(!out.contains('\t'), "{out}");
}

#[test]
fn extract_text_color_can_be_forced_with_short_flag() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	unsafe { std::env::remove_var("NO_COLOR") };
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"-c",
		"--kind",
		"class",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.contains("\x1b["), "expected ANSI color escapes: {out}");
	assert!(out.contains("class"), "{out}");
}

#[test]
fn extract_text_can_emit_full_moniker_uris() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--kind",
		"class",
		"--moniker-format",
		"uri",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.contains("code+moniker://"), "{out}");
	assert!(out.contains("class:Foo"), "{out}");
}

#[test]
fn extract_uri_adds_srcset_segment_for_common_source_layouts() {
	let dir = tempfile::tempdir().expect("tmpdir");
	let path = dir.path().join("src/test/java/com/acme/FooTest.java");
	write_under(
		dir.path(),
		"src/test/java/com/acme/FooTest.java",
		"package com.acme;\nclass FooTest {}\n",
	);
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--kind",
		"class",
		"--moniker-format",
		"uri",
	]);

	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(
		out.contains(
			"code+moniker://./srcset:test/lang:java/package:com/package:acme/module:FooTest/class:FooTest"
		),
		"{out}"
	);
}

#[test]
fn count_only_prints_an_integer() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--count",
	]);
	assert_eq!(exit, Exit::Match);
	let trimmed = out.trim();
	let n: usize = trimmed.parse().expect("expected integer, got {trimmed}");
	assert!(n > 0);
}

#[test]
fn extract_directory_json_is_match_output_not_summary() {
	let dir = tempfile::tempdir().unwrap();
	write_under(dir.path(), "src/a.ts", TS_FIXTURE);
	write_under(dir.path(), "src/b.ts", "export class Beta {}\n");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		dir.path().to_str().unwrap(),
		"--format",
		"json",
		"--all",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
	assert!(v.get("summary").is_none(), "{out}");
	let files = v["files"].as_array().expect("files array");
	assert!(
		files.iter().any(|file| file["matches"]["defs"].is_array()),
		"directory extract JSON should expose per-file matches: {out}"
	);
	assert!(
		files
			.iter()
			.all(|file| file.get("by_def_kind").is_none() && file.get("by_ref_kind").is_none()),
		"directory extract should not return stats summaries: {out}"
	);
}

#[test]
fn stats_json_reports_extraction_metrics() {
	let dir = tempfile::tempdir().unwrap();
	write_under(dir.path(), "src/a.ts", TS_FIXTURE);
	write_under(
		dir.path(),
		"src/lib.rs",
		"mod tests { fn mk() {} fn run() { mk(); } }\n",
	);
	write_under(dir.path(), "README.md", "ignored\n");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"stats",
		dir.path().to_str().unwrap(),
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
	assert_eq!(v["total_files"].as_u64(), Some(2));
	assert_eq!(v["by_lang"]["ts"]["files"].as_u64(), Some(1));
	assert_eq!(v["by_lang"]["rs"]["files"].as_u64(), Some(1));
	assert!(v["by_shape"]["namespace"].as_u64().unwrap() >= 2);
	assert!(v["by_shape"]["callable"].as_u64().unwrap() >= 1);
	assert!(v["by_shape"]["ref"].as_u64().unwrap() >= 1);
	assert!(v["by_kind"]["defs"]["module"].as_u64().unwrap() >= 2);
	assert!(v["timings"]["total_ms"].as_u64().is_some());
	assert!(v["timings"]["scan_ms"].as_u64().is_some());
	assert!(v["timings"]["extract_ms"].as_u64().is_some());
}

#[test]
fn stats_accepts_multiple_source_roots() {
	let dir = tempfile::tempdir().unwrap();
	let service_a = dir.path().join("service-a");
	let service_b = dir.path().join("service-b");
	write_under(&service_a, "src/a.ts", "export class Alpha {}\n");
	write_under(&service_b, "src/b.ts", "export class Beta {}\n");

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"stats",
		service_a.to_str().unwrap(),
		service_b.to_str().unwrap(),
		"--format",
		"json",
	]);

	assert_eq!(exit, Exit::Match, "stderr={err}");
	let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
	assert_eq!(v["total_files"].as_u64(), Some(2));
	assert_eq!(v["by_lang"]["ts"]["files"].as_u64(), Some(2));
	let path = v["path"].as_str().unwrap_or_default();
	assert!(path.contains("service-a"), "{path}");
	assert!(path.contains("service-b"), "{path}");
}

#[test]
fn stats_tsv_is_metrics_only() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"stats",
		dir.path().join("a.ts").to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.contains("files\t1"), "{out}");
	assert!(out.contains("lang\tts\tfiles\t1"), "{out}");
	assert!(out.contains("shape\tcallable\t"), "{out}");
	assert!(!out.contains("code+moniker://"), "{out}");
	assert!(!out.contains("def\t"), "{out}");
}

#[test]
#[cfg(feature = "pretty")]
fn stats_tree_is_human_readable_and_colored_when_requested() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	unsafe { std::env::remove_var("NO_COLOR") };
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"stats",
		dir.path().join("a.ts").to_str().unwrap(),
		"--format",
		"tree",
		"--color",
		"always",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.contains("stats"), "{out}");
	assert!(out.contains("time"), "{out}");
	assert!(out.contains("ms"), "{out}");
	assert!(out.contains("languages"), "{out}");
	assert!(out.contains("shapes"), "{out}");
	assert!(out.contains("\x1b["), "expected ANSI color escapes: {out}");
	assert!(!out.contains("code+moniker://"), "{out}");
	assert!(!out.contains("def\t"), "{out}");
}

#[test]
fn stats_reports_supported_file_read_errors() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("bad.ts");
	std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
	let (exit, out, err) = run_with(vec!["code-moniker", "stats", path.to_str().unwrap()]);
	assert_eq!(exit, Exit::UsageError);
	assert!(out.is_empty(), "{out}");
	assert!(err.contains("cannot extract"), "{err}");
	assert!(err.contains("bad.ts"), "{err}");
}

#[test]
fn quiet_emits_nothing_on_match() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"extract",
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
		"extract",
		path.to_str().unwrap(),
		"--kind",
		"enum_constant",
		"--quiet",
	]);
	assert_eq!(exit, Exit::NoMatch);
}

#[test]
fn class_kind_filter_finds_class_foo() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--format",
		"tsv",
		"--kind",
		"method",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err} stdout={out}");
	let lines: Vec<&str> = out.lines().collect();
	assert!(!lines.is_empty(), "no methods matched");
	for line in &lines {
		assert!(line.contains("class:Foo"), "{line}");
		assert!(line.starts_with("def\t"), "{line}");
	}
}

#[test]
#[cfg(feature = "pretty")]
fn name_regex_filter_keeps_tree_output_ergonomic() {
	let source = r#"
package demo;

interface PaymentResolver {}
interface PaymentService {}
"#;
	let dir = write_fixture("ResolverDemo.java", source);
	let path = dir.path().join("ResolverDemo.java");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--kind",
		"interface",
		"--name",
		"Resolver",
		"--format",
		"tree",
		"--color",
		"never",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err} stdout={out}");
	assert!(out.contains("interface PaymentResolver"), "{out}");
	assert!(!out.contains("PaymentService"), "{out}");
}

#[test]
fn name_regex_accepts_comma_quantifiers() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--kind",
		"class",
		"--name",
		"^Fo{2,3}$",
		"--count",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert_eq!(out.trim(), "1");
}

#[test]
fn json_format_produces_parsable_document() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"extract",
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
		"extract",
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
		"extract",
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
	let (exit, _, err) = run_with(vec!["code-moniker", "extract", path.to_str().unwrap()]);
	assert_eq!(exit, Exit::UsageError);
	assert!(err.contains("unsupported"), "{err}");
}

#[test]
fn malformed_predicate_uri_is_usage_error() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, _, err) = run_with(vec![
		"code-moniker",
		"extract",
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
	assert_eq!(viols[0]["severity"], "error");
}

#[test]
fn check_warn_severity_reports_without_failing() {
	let dir = write_fixture("a.ts", TS_BAD_NAMING);
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[[ts.class.where]]
		id       = "soft-name"
		severity = "warn"
		expr     = "name =~ ^[A-Z][A-Za-z0-9]*$"
		message  = "Class names should be PascalCase."
		"#,
	)
	.unwrap();
	let path = dir.path().join("a.ts");

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::Match, "stdout={out} stderr={err}");
	assert!(out.contains("[ts.class.soft-name] warning:"), "{out}");
	assert!(out.contains("1 warning(s)"), "{out}");
	assert!(out.contains("- ts.class.soft-name: 1 warning(s)"), "{out}");

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out} stderr={err}");
	let v: serde_json::Value = serde_json::from_str(&out).expect("json output");
	assert_eq!(v["summary"]["total_violations"], 1);
	assert_eq!(v["summary"]["total_rule_errors"], 0);
	assert_eq!(v["summary"]["total_warnings"], 1);
	let violation = &v["files"][0]["violations"][0];
	assert_eq!(violation["rule_id"], "ts.class.soft-name");
	assert_eq!(violation["severity"], "warn");
	assert_eq!(v["summary"]["failed_rules"][0]["severity"], "warn");
}

#[test]
fn rules_show_loads_code_smells_sample() {
	let rules_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../docs/cli/check-samples/code-smells-local.toml")
		.canonicalize()
		.expect("code-smells sample path");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"rules",
		"show",
		".",
		"--rules",
		rules_path.to_str().unwrap(),
		"--default-rules",
		"off",
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out}\nstderr={err}");
	let json: serde_json::Value = serde_json::from_str(&out).expect("rules show json");
	assert_eq!(
		json["exclude"]["uris"][0],
		"**/crates/core/tests/fixtures/**"
	);
	assert!(
		json["langs"]
			.as_array()
			.unwrap()
			.iter()
			.flat_map(|lang| lang["rules"].as_array().unwrap())
			.any(
				|rule| rule["rule_id"] == "shape.type.smell-data-clumps-param-names"
					&& rule["severity"] == "warn"
			),
		"{json:#}"
	);
}

#[test]
fn check_project_excludes_configured_uri_globs() {
	let dir = tempfile::tempdir().expect("tmpdir");
	let rules_path = dir.path().join(".code-moniker.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[exclude]
		uris = [
		  "**/crates/core/tests/fixtures/**",
		]

		[[ts.class.where]]
		id = "name-pascalcase"
		expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
		"#,
	)
	.unwrap();
	write_under(dir.path(), "src/good.ts", "class GoodName {}\n");
	write_under(
		dir.path(),
		"crates/core/tests/fixtures/ts/bad.ts",
		TS_BAD_NAMING,
	);

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out}\nstderr={err}");
	let json: serde_json::Value = serde_json::from_str(&out).expect("check JSON");
	assert_eq!(json["summary"]["files_scanned"], 1);
	assert_eq!(json["summary"]["total_violations"], 0);
	let files = json["files"].as_array().unwrap();
	assert_eq!(files.len(), 1, "{json:#}");
	assert!(files[0]["file"].as_str().unwrap().ends_with("src/good.ts"));
	assert!(
		!out.contains("fixtures/ts/bad.ts"),
		"excluded fixture should not be reported: {out}"
	);
}

#[test]
fn check_excluded_single_file_json_is_structured() {
	let dir = tempfile::tempdir().expect("tmpdir");
	let rules_path = dir.path().join(".code-moniker.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[exclude]
		uris = [
		  "**/fixtures/**",
		]

		[[ts.class.where]]
		id = "name-pascalcase"
		expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
		"#,
	)
	.unwrap();
	let excluded = dir.path().join("fixtures/bad.ts");
	write_under(dir.path(), "fixtures/bad.ts", TS_BAD_NAMING);

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		excluded.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out}\nstderr={err}");
	let json: serde_json::Value = serde_json::from_str(&out).expect("check JSON");
	assert_eq!(json["summary"]["files_scanned"], 0);
	assert_eq!(json["summary"]["total_violations"], 0);
	assert!(json["files"].as_array().unwrap().is_empty(), "{json:#}");
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
	assert!(out.contains("Failed rules:"), "{out}");
	assert!(
		out.contains("- ts.class.name-pascalcase: 1 violation(s)"),
		"{out}"
	);
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
fn check_project_continues_on_per_file_errors() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::write(dir.path().join("bad.ts"), TS_BAD_NAMING).unwrap();
	// Non-UTF-8 bytes: `std::fs::read_to_string` rejects this.
	std::fs::write(dir.path().join("broken.ts"), [0xff, 0xfe, 0xff, 0xfe]).unwrap();
	std::fs::write(dir.path().join("good.ts"), "class GoodName {}\n").unwrap();
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
	]);
	// Exit 1: violations + errors both signal an unclean run; exit 2 stays
	// reserved for global usage errors.
	assert_eq!(exit, Exit::NoMatch, "out={out} err={err}");
	assert!(
		out.contains("bad.ts"),
		"bad.ts violation still emitted: {out}"
	);
	assert!(out.contains("ts.class.name-pascalcase"), "{out}");
	assert!(
		err.contains("broken.ts"),
		"broken.ts error on stderr: {err}"
	);
	assert!(
		out.contains("1 file(s) errored"),
		"footer mentions errors: {out}"
	);
	assert!(
		out.contains("Read errors: 1 file(s)."),
		"summary mentions read error count: {out}"
	);
}

#[test]
fn check_project_json_includes_errors_array() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::write(dir.path().join("broken.ts"), [0xff, 0xfe, 0xff]).unwrap();
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
	assert_eq!(v["summary"]["files_with_errors"], 1);
	assert_eq!(v["summary"]["total_errors"], 1);
	let errors = v["errors"].as_array().unwrap();
	assert_eq!(errors.len(), 1);
	assert!(errors[0]["file"].as_str().unwrap().ends_with("broken.ts"));
}

#[test]
fn check_project_path_in_moniker_gates_a_rule() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::create_dir(dir.path().join("strict")).unwrap();
	std::fs::create_dir(dir.path().join("lax")).unwrap();
	std::fs::write(dir.path().join("strict/a.ts"), "class lower_case {}\n").unwrap();
	std::fs::write(dir.path().join("lax/b.ts"), "class lower_case {}\n").unwrap();
	let rules_path = dir.path().join("rules.toml");
	// Replace the preset's name-pascalcase rule with one that only fires
	// inside the `strict` directory — keyed off the path-derived moniker
	// (`**/dir:strict/**` segment).
	std::fs::write(
		&rules_path,
		r#"
		[[ts.class.where]]
		id      = "name-pascalcase"
		expr    = "moniker ~ '**/dir:strict/**' => name =~ ^[A-Z][A-Za-z0-9]*$"
		message = "Strict layer requires PascalCase."
		rationale = "ADR-001: strict layer names are part of the public architecture contract."
		"#,
	)
	.unwrap();
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::NoMatch, "strict/a.ts should violate: {out}");
	assert!(out.contains("strict/a.ts"), "{out}");
	assert!(
		!out.contains("lax/b.ts"),
		"lax/ exempt by path-in-moniker gate: {out}"
	);
	assert!(
		!out.contains("ADR-001"),
		"rationale is rules-show metadata, not check output: {out}"
	);
}

#[test]
fn check_project_file_filter_checks_only_touched_files_with_project_anchors() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::create_dir(dir.path().join("strict")).unwrap();
	std::fs::create_dir(dir.path().join("lax")).unwrap();
	std::fs::write(dir.path().join("strict/a.ts"), "class lower_case {}\n").unwrap();
	std::fs::write(dir.path().join("lax/b.ts"), "class lower_case {}\n").unwrap();
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[[ts.class.where]]
		id      = "strict-name"
		expr    = "moniker ~ '**/dir:strict/**' => name =~ ^[A-Z][A-Za-z0-9]*$"
		message = "Strict layer requires PascalCase."
		"#,
	)
	.unwrap();

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--format",
		"json",
		"--file",
		"lax/b.ts",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out} stderr={err}");
	let json: serde_json::Value = serde_json::from_str(&out).expect("check JSON");
	assert_eq!(json["summary"]["files_scanned"], 1);
	assert_eq!(json["summary"]["total_violations"], 0);
	assert!(
		json["files"][0]["file"]
			.as_str()
			.unwrap()
			.ends_with("lax/b.ts"),
		"{out}"
	);

	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--file",
		"strict/a.ts",
	]);
	assert_eq!(exit, Exit::NoMatch, "strict/a.ts should violate: {out}");
	assert!(out.contains("strict/a.ts"), "{out}");
	assert!(!out.contains("lax/b.ts"), "{out}");
	assert!(out.contains("1 scanned"), "{out}");
}

#[cfg(unix)]
#[test]
fn check_project_file_filter_does_not_read_unfiltered_sources() {
	use std::os::unix::fs::PermissionsExt;

	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::write(dir.path().join("touched.ts"), "class GoodName {}\n").unwrap();
	std::fs::write(dir.path().join("unfiltered.ts"), "class lower_case {}\n").unwrap();
	let mut perms = std::fs::metadata(dir.path().join("unfiltered.ts"))
		.unwrap()
		.permissions();
	perms.set_mode(0o000);
	std::fs::set_permissions(dir.path().join("unfiltered.ts"), perms).unwrap();
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[[ts.class.where]]
		id      = "name-pascalcase"
		expr    = "name =~ ^[A-Z][A-Za-z0-9]*$"
		message = "Class names must be PascalCase."
		"#,
	)
	.unwrap();

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--file",
		"touched.ts",
	]);

	let mut perms = std::fs::metadata(dir.path().join("unfiltered.ts"))
		.unwrap()
		.permissions();
	perms.set_mode(0o644);
	std::fs::set_permissions(dir.path().join("unfiltered.ts"), perms).unwrap();

	assert_eq!(exit, Exit::Match, "stdout={out} stderr={err}");
	assert!(
		out.is_empty(),
		"clean single filtered file should stay quiet: {out}"
	);
	assert!(err.is_empty(), "unfiltered unreadable file was read: {err}");
}

#[test]
fn check_project_file_filter_ignores_unsupported_missing_and_out_of_scope_files_quietly() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::write(dir.path().join("README.md"), "not a source file\n").unwrap();
	let outside = tempfile::NamedTempFile::new().unwrap();

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
		"--file",
		"README.md",
		"--file",
		"missing.ts",
		"--file",
		outside.path().to_str().unwrap(),
	]);

	assert_eq!(exit, Exit::Match, "stdout={out} stderr={err}");
	assert!(
		out.is_empty(),
		"unsupported touched files stay quiet: {out}"
	);
	assert!(err.is_empty(), "stderr={err}");
}

#[test]
fn check_project_file_filter_respects_gitignore() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::create_dir(dir.path().join(".git")).unwrap();
	std::fs::write(dir.path().join(".gitignore"), "ignored.ts\n").unwrap();
	std::fs::write(dir.path().join("ignored.ts"), TS_BAD_NAMING).unwrap();
	std::fs::write(dir.path().join("kept.ts"), TS_BAD_NAMING).unwrap();

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
		"--file",
		"ignored.ts",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out} stderr={err}");
	assert!(
		out.is_empty(),
		"ignored touched file should be quiet: {out}"
	);

	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
		"--file",
		"kept.ts",
	]);
	assert_eq!(exit, Exit::NoMatch, "kept file should be checked: {out}");
	assert!(out.contains("kept.ts"), "{out}");
	assert!(!out.contains("ignored.ts"), "{out}");
}

#[test]
fn check_project_uses_source_context_in_monikers_without_indexing() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::create_dir_all(dir.path().join("src/test/java/com/acme")).unwrap();
	std::fs::create_dir_all(dir.path().join("src/main/java/com/acme")).unwrap();
	std::fs::write(
		dir.path().join("src/test/java/com/acme/lower_bad.java"),
		"package com.acme;\nclass lower_bad {}\n",
	)
	.unwrap();
	std::fs::write(
		dir.path().join("src/main/java/com/acme/lower_bad.java"),
		"package com.acme;\nclass lower_bad {}\n",
	)
	.unwrap();
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[aliases]
		java_test = "moniker ~ '**/srcset:test/**'"

		[[java.class.where]]
		id      = "test-class-pascalcase"
		expr    = "$java_test => name =~ ^[A-Z][A-Za-z0-9]*$"
		message = "Test class names must be PascalCase."
		"#,
	)
	.unwrap();

	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--default-rules",
		"off",
	]);

	assert_eq!(exit, Exit::NoMatch, "test source set should violate: {out}");
	assert!(
		out.contains("src/test/java/com/acme/lower_bad.java"),
		"{out}"
	);
	assert!(
		!out.contains("src/main/java/com/acme/lower_bad.java"),
		"main source set must not be covered by the test alias: {out}"
	);
}

#[test]
fn check_project_file_filter_preserves_java_srcset_discrimination() {
	let dir = tempfile::tempdir().expect("tmpdir");
	write_under(
		dir.path(),
		"src/main/java/com/acme/lower_bad.java",
		"package com.acme;\nclass lower_bad {}\n",
	);
	write_under(
		dir.path(),
		"src/test/java/com/acme/lower_bad.java",
		"package com.acme;\nclass lower_bad {}\n",
	);
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[aliases]
		java_test = "moniker ~ '**/srcset:test/**'"

		[[java.class.where]]
		id      = "test-class-pascalcase"
		expr    = "$java_test => name =~ ^[A-Z][A-Za-z0-9]*$"
		message = "Test class names must be PascalCase."
		"#,
	)
	.unwrap();

	let src_scope = dir.path().join("src");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		src_scope.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--file",
		dir.path()
			.join("src/main/java/com/acme/lower_bad.java")
			.to_str()
			.unwrap(),
	]);
	assert_eq!(exit, Exit::Match, "stdout={out} stderr={err}");
	assert!(
		out.is_empty(),
		"main source file must not match srcset:test: {out}"
	);

	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		src_scope.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--file",
		"src/test/java/com/acme/lower_bad.java",
	]);
	assert_eq!(exit, Exit::NoMatch, "test source set should violate: {out}");
	assert!(
		out.contains("src/test/java/com/acme/lower_bad.java"),
		"{out}"
	);
	assert!(
		!out.contains("src/main/java/com/acme/lower_bad.java"),
		"main source set must not be checked or flagged: {out}"
	);
	assert!(out.contains("java.class.test-class-pascalcase"), "{out}");
}

#[test]
fn check_project_file_filter_does_not_take_srcset_from_parent_directories() {
	let dir = tempfile::tempdir().expect("tmpdir");
	let project = dir.path().join("outer/src/test/project");
	write_under(
		&project,
		"src/main/java/com/acme/lower_bad.java",
		"package com.acme;\nclass lower_bad {}\n",
	);
	let rules_path = project.join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[aliases]
		java_test = "moniker ~ '**/srcset:test/**'"

		[[java.class.where]]
		id      = "test-class-pascalcase"
		expr    = "$java_test => name =~ ^[A-Z][A-Za-z0-9]*$"
		message = "Test class names must be PascalCase."
		"#,
	)
	.unwrap();

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		project.join("src").to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--file",
		project
			.join("src/main/java/com/acme/lower_bad.java")
			.to_str()
			.unwrap(),
	]);

	assert_eq!(exit, Exit::Match, "stdout={out} stderr={err}");
	assert!(
		out.is_empty(),
		"src/main file must not inherit srcset:test from parent path: {out}"
	);
}

#[test]
fn check_project_file_filter_prefers_project_relative_scope_prefixed_path() {
	let dir = tempfile::tempdir().expect("tmpdir");
	write_under(dir.path(), "src/order.ts", "class lower_bad {}\n");
	write_under(dir.path(), "src/src/order.ts", "class GoodName {}\n");
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[[ts.class.where]]
		id      = "name-pascalcase"
		expr    = "name =~ ^[A-Z][A-Za-z0-9]*$"
		message = "Class names must be PascalCase."
		"#,
	)
	.unwrap();

	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().join("src").to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--file",
		"src/order.ts",
	]);

	assert_eq!(
		exit,
		Exit::NoMatch,
		"root-relative src/order.ts should be checked instead of src/src/order.ts: {out}"
	);
	assert!(out.contains("src/order.ts"), "{out}");
	assert!(!out.contains("src/src/order.ts"), "{out}");
	assert!(out.contains("1 scanned"), "{out}");
}

#[test]
fn check_project_file_filter_keeps_absolute_tool_paths_project_anchored() {
	let dir = tempfile::tempdir().expect("tmpdir");
	write_under(
		dir.path(),
		"crates/cli/src/probe.rs",
		"use code_moniker_pg::declare::DeclareSpec;\n",
	);
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[aliases]
		cli = "moniker ~ '**/dir:crates/dir:cli/dir:src/**'"

		[[refs.where]]
		id      = "cli-no-pg"
		expr    = "$cli AND kind = 'imports_symbol' => NOT target ~ '**/external_pkg:code_moniker_pg/**'"
		message = "CLI must not import PG."
		"#,
	)
	.unwrap();

	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--file",
		dir.path().join("crates/cli/src/probe.rs").to_str().unwrap(),
	]);

	assert_eq!(
		exit,
		Exit::NoMatch,
		"absolute tool path should violate: {out}"
	);
	assert!(out.contains("refs.cli-no-pg"), "{out}");
	assert!(
		out.contains("dir:crates/dir:cli/dir:src/module:probe"),
		"source moniker must stay project-relative: {out}"
	);
	assert!(
		!out.contains("dir:Users"),
		"absolute filesystem path leaked into source moniker: {out}"
	);
}

#[test]
fn check_java_spring_proxy_self_invocation_on_real_extraction() {
	let dir = tempfile::tempdir().expect("tmpdir");
	write_under(
		dir.path(),
		"src/main/java/com/acme/service/InvoiceService.java",
		r#"
		package com.acme.service;

		import org.springframework.stereotype.Service;
		import org.springframework.transaction.annotation.Transactional;

		@Service
		public class InvoiceService {
			public void createBatch() {
				createInvoice();
			}

			@Transactional
			public void createInvoice() {}
		}
		"#,
	);
	write_under(
		dir.path(),
		"src/main/java/com/acme/service/AccountService.java",
		r#"
		package com.acme.service;

		import org.springframework.stereotype.Service;
		import org.springframework.transaction.annotation.Transactional;

		@Service
		@Transactional
		public class AccountService {
			public void open() {
				audit();
			}

			public void audit() {}
		}
		"#,
	);
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[[java.method.where]]
		id = "spring-proxy-method-no-self-invocation"
		expr = """
		  any(out_refs, kind = 'annotates' AND target.name = 'Transactional')
		  => none(in_refs,
		       (kind = 'method_call' OR kind = 'calls')
		       AND source.parent = target.parent
		     )
		"""
		message = "method proxy bypass"

		[[java.class.where]]
		id = "spring-proxy-class-no-self-invocation"
		expr = """
		  any(out_refs, kind = 'annotates' AND target.name = 'Transactional')
		  => none(method,
		       any(in_refs,
		         (kind = 'method_call' OR kind = 'calls')
		         AND source.parent = target.parent
		       )
		     )
		"""
		message = "class proxy bypass"
		"#,
	)
	.unwrap();

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--default-rules",
		"off",
	]);

	assert_eq!(exit, Exit::NoMatch, "stdout={out}\nstderr={err}");
	assert!(
		out.contains("java.method.spring-proxy-method-no-self-invocation"),
		"{out}"
	);
	assert!(
		out.contains("method `createInvoice`"),
		"method-level annotation should be flagged: {out}"
	);
	assert!(
		out.contains("java.class.spring-proxy-class-no-self-invocation"),
		"{out}"
	);
	assert!(
		out.contains("class `AccountService`"),
		"class-level annotation should be flagged: {out}"
	);
}

#[test]
fn check_project_cross_layer_import_violation() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::create_dir_all(dir.path().join("src/core")).unwrap();
	std::fs::write(
		dir.path().join("src/core/bad.rs"),
		"use pgrx::prelude::*;\npub fn foo() {}\n",
	)
	.unwrap();
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[aliases]
		core = "moniker ~ '**/dir:src/dir:core/**'"

		[[refs.where]]
		id   = "core-no-pgrx"
		expr = "$core AND kind = 'imports_symbol' => NOT target ~ '**/external_pkg:pgrx/**'"
		"#,
	)
	.unwrap();
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::NoMatch, "core/bad.rs imports pgrx: {out}");
	assert!(out.contains("core-no-pgrx"), "{out}");
	assert!(out.contains("bad.rs"), "{out}");
}

#[test]
fn check_max_violations_prints_largest_rule_group_by_path() {
	let dir = tempfile::tempdir().expect("tmpdir");
	write_under(dir.path(), "src/c.ts", "class Charlie {}\n");
	write_under(dir.path(), "src/a.ts", "class Alpha {}\n");
	write_under(dir.path(), "lib/b.ts", "class Bravo {}\n");
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		default_rules = false

		[[ts.class.where]]
		id = "large"
		expr = "name =~ ^Never$"

		[[ts.class.where]]
		id = "small"
		expr = "moniker ~ '**/dir:src/**' => name =~ ^Never$"
		"#,
	)
	.unwrap();

	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--max-violations",
		"2",
	]);

	assert_eq!(exit, Exit::NoMatch);
	assert!(
		out.contains("Showing 2 of 5 violation(s) from largest rule group `ts.class.large`."),
		"{out}"
	);
	let lib_b = out.find("lib/b.ts").expect("first selected path");
	let src_a = out.find("src/a.ts").expect("second selected path");
	assert!(lib_b < src_a, "{out}");
	assert!(!out.contains("src/c.ts:L1-L1 [ts.class.large]"), "{out}");
	assert!(!out.contains("[ts.class.small]"), "{out}");
	assert!(out.contains("5 violation(s) across 3 file(s)"), "{out}");
	assert!(out.contains("- ts.class.large: 3 violation(s)"), "{out}");
	assert!(out.contains("- ts.class.small: 2 violation(s)"), "{out}");
}

#[test]
fn check_report_warns_when_implication_antecedent_never_matches() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::create_dir_all(dir.path().join("src/core")).unwrap();
	std::fs::write(
		dir.path().join("src/core/bad.ts"),
		"import { apiRouter } from '../api/index.js';\nexport const x = apiRouter;\n",
	)
	.unwrap();
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[aliases]
		source_core = "source ~ '**/dir:src/dir:core/**'"

		[[refs.where]]
		id      = "core-depends-only-on-core"
		expr    = "$source_core => target ~ '**/dir:core/**'"
		message = "Core code may only depend on core internals."
		"#,
	)
	.unwrap();
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().join("src").to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--report",
	]);
	assert_eq!(
		exit,
		Exit::Match,
		"bad alias should hide the violation: {out}"
	);
	assert!(out.contains("Rule report"), "{out}");
	assert!(out.contains("refs.core-depends-only-on-core"), "{out}");
	assert!(out.contains("matches=0"), "{out}");
	assert!(out.contains("antecedent_matches=0"), "{out}");
	assert!(out.contains("warning: antecedent never matched"), "{out}");
}

#[test]
fn check_report_uses_post_suppression_violation_counts() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::write(
		dir.path().join("a.ts"),
		"// code-moniker: ignore[name-pascalcase]\nclass lower_bad {}\n",
	)
	.unwrap();
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[[ts.class.where]]
		id   = "name-pascalcase"
		expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
		"#,
	)
	.unwrap();
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().join("a.ts").to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--format",
		"json",
		"--report",
	]);
	assert_eq!(exit, Exit::Match, "{out}");
	let v: serde_json::Value = serde_json::from_str(&out).expect("json output");
	assert_eq!(v["summary"]["total_violations"], 0);
	let report = v["rule_report"].as_array().unwrap();
	let class_rule = report
		.iter()
		.find(|item| item["rule_id"] == "ts.class.name-pascalcase")
		.expect("class rule report");
	assert_eq!(class_rule["violations"], 0);
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
	assert!(out.contains("elapsed "), "{out}");
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
	assert_eq!(v["summary"]["total_errors"], 0);
	assert!(v["summary"]["elapsed_ms"].as_u64().is_some());
	let failed_rules = v["summary"]["failed_rules"].as_array().unwrap();
	assert_eq!(failed_rules.len(), 1);
	assert_eq!(failed_rules[0]["rule_id"], "ts.class.name-pascalcase");
	assert_eq!(failed_rules[0]["violations"], 1);
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
		"--report",
	]);
	assert_eq!(exit, Exit::NoMatch);
	assert!(out.contains("ts.class.require_doc_comment"), "{out}");
	assert!(out.contains("Rule report"), "{out}");
	assert!(out.contains("violations=1"), "{out}");
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
fn check_default_rules_can_be_disabled() {
	let dir = write_fixture("a.ts", "function helper() {}\n");
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
		"--default-rules",
		"off",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out}\nstderr={err}");
	assert!(
		out.trim().is_empty(),
		"no embedded default rules should run: {out}"
	);
}

#[test]
fn check_default_rules_can_be_disabled_from_config() {
	let dir = write_fixture("a.ts", "function helper() {}\n");
	let rules_path = dir.path().join(".code-moniker.toml");
	std::fs::write(&rules_path, "default_rules = false\n").unwrap();
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::Match, "stdout={out}\nstderr={err}");
	assert!(
		out.trim().is_empty(),
		"no embedded default rules should run: {out}"
	);
}

#[test]
fn check_default_rules_on_overrides_disabled_config() {
	let dir = write_fixture("a.ts", "function helper() {}\n");
	let rules_path = dir.path().join(".code-moniker.toml");
	std::fs::write(&rules_path, "default_rules = false\n").unwrap();
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--default-rules",
		"on",
	]);
	assert_eq!(exit, Exit::NoMatch);
	assert!(out.contains("ts.function.no-placeholder-names"), "{out}");
}

#[test]
fn check_loads_enabled_fragment_rules_from_rules_root() {
	let dir = tempfile::tempdir().expect("tmpdir");
	write_under(dir.path(), "src/a.ts", "class Foo {}\n");
	write_under(
		dir.path(),
		".code-moniker.toml",
		r#"
		default_rules = false
		"#,
	);
	write_under(
		dir.path(),
		"src/code-moniker.fragment.toml",
		r#"
		fragment = "local"

		[aliases]
		target_class = "name = 'X'"

		[[ts.class.where]]
		id = "class-name-x"
		expr = "$target_class"
		"#,
	);

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		dir.path().join(".code-moniker.toml").to_str().unwrap(),
	]);

	assert_eq!(exit, Exit::NoMatch, "stdout={out}\nstderr={err}");
	assert!(out.contains("ts.class.local.class-name-x"), "{out}");
}

#[test]
fn check_ignores_disabled_fragment_rules_but_rules_show_reports_them() {
	let dir = tempfile::tempdir().expect("tmpdir");
	write_under(dir.path(), "src/a.ts", "class Foo {}\n");
	write_under(
		dir.path(),
		".code-moniker.toml",
		r#"
		default_rules = false
		"#,
	);
	write_under(
		dir.path(),
		"src/code-moniker.fragment.toml",
		r#"
		fragment = "local"
		enabled = false

		[[ts.class.where]]
		id = "class-name-x"
		expr = "name = 'X'"
		"#,
	);

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		dir.path().to_str().unwrap(),
		"--rules",
		dir.path().join(".code-moniker.toml").to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::Match, "stdout={out}\nstderr={err}");
	assert!(
		out.trim().is_empty(),
		"disabled fragment should not run: {out}"
	);

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"rules",
		"show",
		dir.path().to_str().unwrap(),
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out}\nstderr={err}");
	let json: serde_json::Value = serde_json::from_str(&out).expect("rules show json");
	let fragment = &json["fragments"].as_array().unwrap()[0];
	assert_eq!(fragment["id"], "local");
	assert_eq!(fragment["enabled"], false);
	assert_eq!(fragment["declared_rules"], 1);
	assert_eq!(fragment["active_rules"], 0);
	assert!(
		!json["langs"]
			.as_array()
			.unwrap()
			.iter()
			.flat_map(|lang| lang["rules"].as_array().unwrap())
			.any(|rule| rule["rule_id"] == "ts.class.local.class-name-x"),
		"{json:#}"
	);
}

#[test]
fn rules_show_profile_recomputes_fragment_active_rules() {
	let dir = tempfile::tempdir().expect("tmpdir");
	write_under(
		dir.path(),
		".code-moniker.toml",
		r#"
		default_rules = false

		[profiles.none]
		disable = ["^ts\\.class\\.local\\.class-name-x$"]
		"#,
	);
	write_under(
		dir.path(),
		"src/code-moniker.fragment.toml",
		r#"
		fragment = "local"

		[[ts.class.where]]
		id = "class-name-x"
		expr = "name = 'X'"
		"#,
	);

	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"rules",
		"show",
		dir.path().to_str().unwrap(),
		"--profile",
		"none",
		"--format",
		"json",
	]);

	assert_eq!(exit, Exit::Match, "stdout={out}\nstderr={err}");
	let json: serde_json::Value = serde_json::from_str(&out).expect("rules show json");
	let fragment = &json["fragments"].as_array().unwrap()[0];
	assert_eq!(fragment["declared_rules"], 1);
	assert_eq!(fragment["active_rules"], 0);
	assert_eq!(json["total_rules"], 0);
}

#[test]
fn check_report_keeps_per_lang_ref_rule_ids() {
	let dir = write_fixture("a.ts", "import { Foo } from './foo';\nclass GoodName {}\n");
	let path = dir.path().join("a.ts");
	let rules_path = dir.path().join("rules.toml");
	std::fs::write(
		&rules_path,
		r#"
		[[refs.where]]
		id = "same"
		expr = "kind != 'imports_symbol'"

		[[ts.refs.where]]
		id = "same"
		expr = "kind != 'imports_symbol'"
		"#,
	)
	.unwrap();

	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		rules_path.to_str().unwrap(),
		"--default-rules",
		"off",
		"--report",
	]);

	assert_eq!(exit, Exit::NoMatch);
	assert!(out.contains("refs.same"), "{out}");
	assert!(out.contains("ts.refs.same"), "{out}");
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
fn check_java_default_allows_screaming_field_constants() {
	let dir = write_fixture(
		"App.java",
		r#"
		public class App {
			private static final String DEFAULT_REGION = "EU";
			private int retryCount;

			public String displayName() {
				return DEFAULT_REGION + retryCount;
			}
		}
		"#,
	);
	let path = dir.path().join("App.java");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"check",
		path.to_str().unwrap(),
		"--rules",
		"/no/such/file.toml",
	]);
	assert_eq!(exit, Exit::Match, "stdout={out}\nstderr={err}");
	assert!(!out.contains("java.field"), "{out}");
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
fn langs_no_arg_lists_every_supported_tag() {
	let (exit, out, err) = run_with(vec!["code-moniker", "langs"]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	let tags: Vec<&str> = out.lines().collect();
	for expected in ["rs", "ts", "java", "python", "go", "cs", "sql"] {
		assert!(tags.contains(&expected), "missing `{expected}` in {tags:?}");
	}
}

#[test]
fn langs_rs_groups_kinds_by_shape_with_visibilities() {
	let (exit, out, err) = run_with(vec!["code-moniker", "langs", "rs"]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.starts_with("lang: rs\n"), "{out}");
	let line_with = |label: &str| -> &str {
		out.lines()
			.find(|l| l.trim_start().starts_with(label))
			.unwrap_or_else(|| panic!("no `{label}` line in:\n{out}"))
	};
	for (label, must_contain) in [
		("namespace:", &["impl", "module"][..]),
		("type:", &["struct", "enum", "trait"][..]),
		("callable:", &["fn", "method"][..]),
		("value:", &["const", "static", "local", "param"][..]),
		("annotation:", &["comment"][..]),
		(
			"ref:",
			&["calls", "imports_symbol", "extends", "annotates"][..],
		),
	] {
		let line = line_with(label);
		for needle in must_contain {
			assert!(
				line.contains(needle),
				"`{label}` missing `{needle}`: {line}"
			);
		}
	}
	let shape_order: Vec<usize> = [
		"namespace:",
		"type:",
		"callable:",
		"value:",
		"annotation:",
		"ref:",
	]
	.iter()
	.map(|l| out.find(l).unwrap_or_else(|| panic!("no `{l}` in:\n{out}")))
	.collect();
	assert!(
		shape_order.windows(2).all(|w| w[0] < w[1]),
		"shapes are not in canonical order in:\n{out}"
	);
	assert!(
		out.contains("visibilities: public, private, module"),
		"{out}"
	);
}

#[test]
fn langs_sql_reports_empty_visibilities() {
	let (exit, out, err) = run_with(vec!["code-moniker", "langs", "sql"]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.contains("visibilities: (none"), "{out}");
}

#[test]
fn langs_unknown_tag_is_usage_error() {
	let (exit, _, err) = run_with(vec!["code-moniker", "langs", "cobol"]);
	assert_eq!(exit, Exit::UsageError);
	assert!(err.contains("unknown language `cobol`"), "{err}");
	assert!(err.contains("known:"), "{err}");
}

#[test]
fn shape_callable_filter_picks_methods_only() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--format",
		"tsv",
		"--shape",
		"callable",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	for line in out.lines() {
		assert!(line.starts_with("def\t"), "callables are defs: {line}");
		assert!(
			line.contains("method:"),
			"only method kinds remain in this fixture: {line}"
		);
	}
}

#[test]
fn shape_comma_list_combines_families() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--shape",
		"type,callable",
	]);
	assert_eq!(exit, Exit::Match);
	assert!(out.contains("class:Foo"), "type kept: {out}");
	assert!(out.contains("class:Bar"), "type kept: {out}");
	assert!(out.contains("method:"), "callable kept: {out}");
	assert!(!out.contains("comment:"), "annotation excluded: {out}");
}

#[test]
fn shape_and_kind_compose_as_and() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, _) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--kind",
		"method",
		"--shape",
		"type",
		"--count",
	]);
	assert_eq!(
		exit,
		Exit::NoMatch,
		"AND of disjoint filters yields zero matches"
	);
	assert_eq!(
		out.trim(),
		"0",
		"method is callable, AND shape=type empties: {out}"
	);
}

#[test]
fn kind_comma_list_is_equivalent_to_repeated_flag() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (_, out_csv, _) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--kind",
		"class,method",
	]);
	let (_, out_rep, _) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--kind",
		"class",
		"--kind",
		"method",
	]);
	assert_eq!(out_csv, out_rep, "comma-list and repeated --kind diverged");
}

#[test]
fn shapes_command_documents_every_canonical_shape() {
	let (exit, out, _) = run_with(vec!["code-moniker", "shapes"]);
	assert_eq!(exit, Exit::Match);
	for name in [
		"namespace",
		"type",
		"callable",
		"value",
		"annotation",
		"ref",
	] {
		assert!(
			out.lines().any(|l| l.trim_start().starts_with(name)),
			"shape `{name}` missing from `shapes` output:\n{out}"
		);
	}
	assert!(
		out.contains("langs <TAG>"),
		"output should cross-reference `langs` for the per-language mapping:\n{out}"
	);
}

#[test]
fn shapes_json_is_parsable_and_complete() {
	let (exit, out, _) = run_with(vec!["code-moniker", "shapes", "--format", "json"]);
	assert_eq!(exit, Exit::Match);
	let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
	let arr = v.as_array().expect("top-level array");
	assert_eq!(arr.len(), 6);
	let names: Vec<&str> = arr.iter().map(|e| e["name"].as_str().unwrap()).collect();
	assert_eq!(
		names,
		vec![
			"namespace",
			"type",
			"callable",
			"value",
			"annotation",
			"ref"
		]
	);
}

#[test]
fn langs_json_format_emits_kinds_array() {
	let (exit, out, _) = run_with(vec!["code-moniker", "langs", "rs", "--format", "json"]);
	assert_eq!(exit, Exit::Match);
	let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
	assert_eq!(v["lang"], "rs");
	let kinds = v["kinds"].as_array().expect("kinds is array");
	assert!(
		kinds
			.iter()
			.any(|k| k["name"] == "fn" && k["shape"] == "callable")
	);
	assert!(
		kinds
			.iter()
			.any(|k| k["name"] == "calls" && k["shape"] == "ref")
	);
}

#[test]
fn manifest_subcommand_emits_package_moniker_per_dep() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(
		dir.path().join("package.json"),
		r#"{"name":"demo","version":"0.1.0","dependencies":{"react":"^18"}}"#,
	)
	.unwrap();
	let path = dir.path().join("package.json");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"manifest",
		path.to_str().unwrap(),
		"--format",
		"json",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
	let rows = v.as_array().expect("array");
	assert!(rows.iter().any(|r| r["import_root"] == "react"
		&& r["package_moniker"] == "code+moniker://./external_pkg:react"));
}

#[test]
fn manifest_subcommand_walks_directory() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(
		dir.path().join("Cargo.toml"),
		"[package]\nname=\"demo\"\nversion=\"0\"\n\n[dependencies]\nserde = \"1\"\n",
	)
	.unwrap();
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"manifest",
		dir.path().to_str().unwrap(),
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.contains("external_pkg:serde"), "{out}");
	assert!(out.contains("\tCargo.toml\t"), "{out}");
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

#[test]
fn project_flag_overrides_anchor_project_segment() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--project",
		"my-app",
		"--moniker-format",
		"uri",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	for line in out.lines() {
		assert!(
			line.contains("code+moniker://my-app/"),
			"expected anchor project `my-app`, got: {line}"
		);
		assert!(
			!line.contains("code+moniker://./"),
			"default `.` anchor leaked: {line}"
		);
	}
}

#[test]
fn cache_respects_project_context() {
	let dir = write_fixture("a.ts", "export class Foo {}\n");
	let path = dir.path().join("a.ts");
	let cache = dir.path().join(".cache");
	let (first_exit, first_out, first_err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--cache",
		cache.to_str().unwrap(),
		"--project",
		"one",
		"--moniker-format",
		"uri",
		"--kind",
		"class",
	]);
	assert_eq!(first_exit, Exit::Match, "stderr={first_err}");
	assert!(first_out.contains("code+moniker://one/"), "{first_out}");

	let (second_exit, second_out, second_err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--cache",
		cache.to_str().unwrap(),
		"--project",
		"two",
		"--moniker-format",
		"uri",
		"--kind",
		"class",
	]);
	assert_eq!(second_exit, Exit::Match, "stderr={second_err}");
	assert!(second_out.contains("code+moniker://two/"), "{second_out}");
	assert!(!second_out.contains("code+moniker://one/"), "{second_out}");
}

#[test]
fn project_flag_composes_with_scheme() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--scheme",
		"esac+moniker://",
		"--project",
		"my-app",
		"--moniker-format",
		"uri",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	for line in out.lines() {
		assert!(
			line.contains("esac+moniker://my-app/"),
			"expected `esac+moniker://my-app/` prefix, got: {line}"
		);
	}
}

#[test]
fn project_flag_default_keeps_dot_anchor() {
	let dir = write_fixture("a.ts", "export class Foo {}\n");
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec![
		"code-moniker",
		"extract",
		path.to_str().unwrap(),
		"--moniker-format",
		"uri",
	]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(
		out.lines().any(|l| l.contains("code+moniker://./")),
		"default project `.` expected in: {out}"
	);
}
