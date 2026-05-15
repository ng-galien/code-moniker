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
fn no_predicate_dumps_full_graph_as_tsv() {
	let dir = write_fixture("a.ts", TS_FIXTURE);
	let path = dir.path().join("a.ts");
	let (exit, out, err) = run_with(vec!["code-moniker", "extract", path.to_str().unwrap()]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(out.lines().any(|l| l.starts_with("def\t")), "{out}");
	assert!(out.contains("class:Foo"), "{out}");
	assert!(out.contains("class:Bar"), "{out}");
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
	let (exit, out, err) = run_with(vec!["code-moniker", "extract", path.to_str().unwrap()]);
	assert_eq!(exit, Exit::Match, "stderr={err}");
	assert!(
		out.lines().any(|l| l.contains("code+moniker://./")),
		"default project `.` expected in: {out}"
	);
}
