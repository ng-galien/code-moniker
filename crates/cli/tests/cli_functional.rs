//! Functional CLI tests that spawn the packaged binary instead of calling
//! `cli::run` directly. These catch clap/bin wiring, real stdout/stderr, and
//! process exit-code regressions.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

struct CmdOut {
	status: ExitStatus,
	stdout: String,
	stderr: String,
}

fn run<I, S>(args: I) -> CmdOut
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.args(args)
		.output()
		.expect("spawn code-moniker binary");
	CmdOut {
		status: output.status,
		stdout: String::from_utf8(output.stdout).expect("stdout utf8"),
		stderr: String::from_utf8(output.stderr).expect("stderr utf8"),
	}
}

fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
	let path = dir.join(name);
	let mut f = std::fs::File::create(&path).expect("create fixture");
	f.write_all(body.as_bytes()).expect("write fixture");
	path
}

fn assert_code(out: &CmdOut, code: i32) {
	assert_eq!(
		out.status.code(),
		Some(code),
		"stdout:\n{}\nstderr:\n{}",
		out.stdout,
		out.stderr
	);
}

const TS_SAMPLE: &str = r#"// TODO header
export class Foo {
  bar(s: string): void {
    // nested comment
  }
}

class Bar extends Foo {}
"#;

#[test]
fn binary_help_version_and_usage_errors_are_wired() {
	let out = run(["--version"]);
	assert_code(&out, 0);
	assert_eq!(
		out.stdout.trim(),
		format!("code-moniker {}", env!("CARGO_PKG_VERSION"))
	);
	assert!(out.stderr.is_empty(), "stderr: {}", out.stderr);

	let out = run(["--help"]);
	assert_code(&out, 0);
	assert!(out.stdout.contains("Usage: code-moniker"), "{}", out.stdout);
	for verb in ["extract", "check", "langs", "shapes"] {
		assert!(
			out.stdout.contains(verb),
			"missing `{verb}`: {}",
			out.stdout
		);
	}

	let out = run(["check", "--help"]);
	assert_code(&out, 0);
	assert!(
		out.stdout.contains("Usage: code-moniker check"),
		"{}",
		out.stdout
	);
	assert!(out.stdout.contains("--rules"), "{}", out.stdout);

	let out = run(std::iter::empty::<&str>());
	assert_code(&out, 2);
	assert!(out.stdout.is_empty(), "stdout: {}", out.stdout);
	assert!(
		out.stderr.contains("requires a subcommand") || out.stderr.contains("Usage:"),
		"expected clap subcommand-required error; got: {}",
		out.stderr
	);
}

#[test]
fn binary_extracts_filters_formats_and_sets_exit_codes() {
	let dir = tempfile::tempdir().expect("tmpdir");
	let file = write_file(dir.path(), "sample.ts", TS_SAMPLE);

	let out = run(["extract".as_ref(), file.as_os_str()]);
	assert_code(&out, 0);
	assert!(out.stdout.contains("def\t"), "{}", out.stdout);
	assert!(out.stdout.contains("class:Foo"), "{}", out.stdout);
	assert!(out.stdout.contains("class:Bar"), "{}", out.stdout);

	let out = run([
		"extract".as_ref(),
		file.as_os_str(),
		"--kind".as_ref(),
		"comment".as_ref(),
		"--count".as_ref(),
	]);
	assert_code(&out, 0);
	assert_eq!(out.stdout.trim(), "2");

	let out = run([
		"extract".as_ref(),
		file.as_os_str(),
		"--kind".as_ref(),
		"enum_constant".as_ref(),
		"--quiet".as_ref(),
	]);
	assert_code(&out, 1);
	assert!(out.stdout.is_empty(), "stdout: {}", out.stdout);
	assert!(out.stderr.is_empty(), "stderr: {}", out.stderr);

	let out = run([
		"extract".as_ref(),
		file.as_os_str(),
		"--kind".as_ref(),
		"does_not_exist".as_ref(),
		"--quiet".as_ref(),
	]);
	assert_code(&out, 2);
	assert!(
		out.stderr.contains("unknown --kind does_not_exist"),
		"expected validation error on stderr; got: {}",
		out.stderr
	);

	let out = run([
		"extract".as_ref(),
		file.as_os_str(),
		"--format".as_ref(),
		"json".as_ref(),
		"--with-text".as_ref(),
	]);
	assert_code(&out, 0);
	let json: serde_json::Value = serde_json::from_str(&out.stdout).expect("extract JSON");
	assert_eq!(json["lang"], "ts");
	let defs = json["matches"]["defs"].as_array().expect("defs array");
	assert!(defs.iter().any(|d| {
		d["kind"] == "comment"
			&& d["text"]
				.as_str()
				.is_some_and(|text| text.contains("TODO header"))
	}));
	let foo_uri = defs
		.iter()
		.find(|d| {
			d["kind"] == "class"
				&& d["moniker"]
					.as_str()
					.is_some_and(|m| m.contains("class:Foo"))
		})
		.and_then(|d| d["moniker"].as_str())
		.expect("Foo class moniker");
	let predicate = format!("<@ {foo_uri}");
	let out = run([
		"extract".as_ref(),
		file.as_os_str(),
		"--where".as_ref(),
		predicate.as_ref(),
		"--kind".as_ref(),
		"method".as_ref(),
	]);
	assert_code(&out, 0);
	assert!(out.stdout.contains("method:bar"), "{}", out.stdout);
}

#[test]
fn binary_extension_contract_matches_extract_behavior() {
	let dir = tempfile::tempdir().expect("tmpdir");
	let txt = write_file(dir.path(), "a.txt", "hello\n");
	let mjs = write_file(
		dir.path(),
		"module.mjs",
		"export function GoodName() { return 1; }\n",
	);
	let pyi = write_file(dir.path(), "types.pyi", "class GoodName: ...\n");

	let out = run(["extract".as_ref(), txt.as_os_str()]);
	assert_code(&out, 2);
	assert!(
		out.stderr.contains("unsupported file extension"),
		"{}",
		out.stderr
	);

	for file in [mjs, pyi] {
		let out = run(["extract".as_ref(), file.as_os_str(), "--quiet".as_ref()]);
		assert_code(&out, 0);
		assert!(out.stdout.is_empty(), "stdout: {}", out.stdout);
		assert!(out.stderr.is_empty(), "stderr: {}", out.stderr);
	}
}

#[test]
fn binary_check_reports_text_json_and_usage_errors() {
	let dir = tempfile::tempdir().expect("tmpdir");
	let clean = write_file(dir.path(), "clean.ts", "class GoodName {}\n");
	let bad = write_file(dir.path(), "bad.ts", "class lower_case_class {}\n");

	let out = run([
		"check".as_ref(),
		clean.as_os_str(),
		"--rules".as_ref(),
		"/no/such/file.toml".as_ref(),
	]);
	assert_code(&out, 0);
	assert!(out.stdout.is_empty(), "stdout: {}", out.stdout);
	assert!(out.stderr.is_empty(), "stderr: {}", out.stderr);

	let out = run([
		"check".as_ref(),
		bad.as_os_str(),
		"--rules".as_ref(),
		"/no/such/file.toml".as_ref(),
	]);
	assert_code(&out, 1);
	assert!(
		out.stdout.contains("ts.class.name-pascalcase"),
		"{}",
		out.stdout
	);
	assert!(out.stdout.contains("L1-L1"), "{}", out.stdout);

	let out = run([
		"check".as_ref(),
		bad.as_os_str(),
		"--rules".as_ref(),
		"/no/such/file.toml".as_ref(),
		"--format".as_ref(),
		"json".as_ref(),
	]);
	assert_code(&out, 1);
	let json: serde_json::Value = serde_json::from_str(&out.stdout).expect("check JSON");
	assert_eq!(json["summary"]["files_scanned"], 1);
	assert_eq!(json["summary"]["files_with_violations"], 1);
	assert_eq!(
		json["files"][0]["violations"][0]["rule_id"],
		"ts.class.name-pascalcase"
	);

	let rules = write_file(
		dir.path(),
		"bad-rules.toml",
		r#"
[[ts.classs.where]]
expr = "name =~ ^X"
"#,
	);
	let out = run([
		"check".as_ref(),
		clean.as_os_str(),
		"--rules".as_ref(),
		rules.as_os_str(),
	]);
	assert_code(&out, 2);
	assert!(out.stderr.contains("classs"), "{}", out.stderr);
}

#[test]
fn binary_check_project_respects_gitignore() {
	let dir = tempfile::tempdir().expect("tmpdir");
	std::fs::create_dir(dir.path().join(".git")).expect("mkdir .git");
	write_file(dir.path(), ".gitignore", "ignored.ts\n");
	write_file(dir.path(), "scanned.ts", "class lower_scanned {}\n");
	write_file(dir.path(), "ignored.ts", "class lower_ignored {}\n");

	let out = run([
		"check".as_ref(),
		dir.path().as_os_str(),
		"--rules".as_ref(),
		"/no/such/file.toml".as_ref(),
	]);
	assert_code(&out, 1);
	assert!(out.stdout.contains("scanned.ts"), "{}", out.stdout);
	assert!(
		!out.stdout.contains("ignored.ts"),
		"ignored file leaked into output: {}",
		out.stdout
	);
}

#[test]
fn binary_check_rejects_nested_rust_comments() {
	let dir = tempfile::tempdir().expect("tmpdir");
	let rules = write_file(
		dir.path(),
		"rules.toml",
		r#"
[[rust.comment.where]]
id      = "no-nested-comments"
expr    = "parent.kind = 'module' OR text =~ '^//\\s*SAFETY:'"
message = "nested comment"
"#,
	);
	let file = write_file(
		dir.path(),
		"comments.rs",
		r#"
// module comment is ok
struct Foo {
    // field comment
    value: i32,
}

impl Foo {
    // impl comment
    fn new() -> Self {
        // method comment
        Self { value: 0 }
    }
}

trait Bar {
    // trait comment
    fn bar(&self);
}

enum Baz {
    // enum comment
    A,
}
"#,
	);

	let out = run([
		"check".as_ref(),
		file.as_os_str(),
		"--rules".as_ref(),
		rules.as_os_str(),
	]);
	assert_code(&out, 1);
	assert!(out.stdout.contains("L4-L4"), "{}", out.stdout);
	assert!(out.stdout.contains("L9-L9"), "{}", out.stdout);
	assert!(out.stdout.contains("L11-L11"), "{}", out.stdout);
	assert!(out.stdout.contains("L17-L17"), "{}", out.stdout);
	assert!(out.stdout.contains("L22-L22"), "{}", out.stdout);
	assert!(!out.stdout.contains("L2-L2"), "{}", out.stdout);
}
