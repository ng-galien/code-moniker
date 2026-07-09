//! End-to-end tests for `code-moniker diff`. Each test builds a real git
//! repository in a tempdir, drives `cli::run` in process, and asserts on
//! the rendered symbol-level facts.

use std::path::Path;
use std::process::Command;

use clap::Parser;
use code_moniker_cli::{self as cli, Cli, Exit};

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

fn write(root: &Path, rel: &str, body: &str) {
	let path = root.join(rel);
	if let Some(parent) = path.parent() {
		std::fs::create_dir_all(parent).unwrap();
	}
	std::fs::write(path, body).unwrap();
}

fn git(root: &Path, args: &[&str]) {
	let output = Command::new("git")
		.arg("-C")
		.arg(root)
		.args(args)
		.output()
		.unwrap_or_else(|e| panic!("cannot run git {args:?}: {e}"));
	assert!(
		output.status.success(),
		"git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);
}

fn moved_repo() -> tempfile::TempDir {
	let tmp = tempfile::tempdir().unwrap();
	git(tmp.path(), &["init"]);
	git(tmp.path(), &["config", "user.email", "cm@example.test"]);
	git(tmp.path(), &["config", "user.name", "Code Moniker"]);
	write(tmp.path(), "src/lib.rs", "mod util;\nmod consumer;\n");
	write(
		tmp.path(),
		"src/util.rs",
		"pub fn assist() { work(); }\npub fn sidekick() { rest(); }\n",
	);
	write(
		tmp.path(),
		"src/consumer.rs",
		"use crate::util::assist;\n\npub fn caller() { assist(); }\npub fn edited() -> u32 { 1 }\n",
	);
	git(tmp.path(), &["add", "."]);
	git(tmp.path(), &["commit", "-m", "initial"]);
	git(tmp.path(), &["mv", "src/util.rs", "src/support.rs"]);
	write(tmp.path(), "src/lib.rs", "mod support;\nmod consumer;\n");
	write(
		tmp.path(),
		"src/consumer.rs",
		"use crate::support::assist;\n\npub fn caller() { assist(); }\npub fn edited() -> u32 { 2 }\n",
	);
	tmp
}

#[test]
fn diff_text_reports_a_pure_move_and_the_isolated_edit() {
	let tmp = moved_repo();
	let root = tmp.path().to_str().unwrap();

	let (exit, out, err) = run_with(vec!["code-moniker", "diff", root]);

	assert_eq!(exit, Exit::Match, "stderr: {err}");
	assert!(
		out.contains("src/util.rs -> src/support.rs  moved"),
		"missing pure move heading:\n{out}"
	);
	assert!(
		out.contains("= 2 symbol(s) moved, no other facts"),
		"moved symbols must collapse:\n{out}"
	);
	assert!(
		out.contains("~ fn edited()  body-modified"),
		"edited symbol must surface:\n{out}"
	);
	assert!(
		out.contains("residual files 0"),
		"every hunk must be explained:\n{out}"
	);
}

#[test]
fn diff_json_carries_the_versioned_fact_schema() {
	let tmp = moved_repo();
	let root = tmp.path().to_str().unwrap();

	let (exit, out, err) = run_with(vec!["code-moniker", "diff", root, "--format", "json"]);

	assert_eq!(exit, Exit::Match, "stderr: {err}");
	let payload: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
	assert_eq!(payload["schema"], "code-moniker.diff/1");
	assert_eq!(payload["scope"], "HEAD..worktree");
	let moved = payload["files"]
		.as_array()
		.expect("files array")
		.iter()
		.find(|file| file["disposition"] == "moved")
		.expect("moved file entry");
	assert_eq!(moved["old_path"], "src/util.rs");
	assert_eq!(moved["new_path"], "src/support.rs");
	assert_eq!(moved["moved_symbols"], 2);
	assert_eq!(moved["coverage"]["explained"], true);
	let kinds: Vec<&str> = payload["symbol_changes"]
		.as_array()
		.expect("symbol changes")
		.iter()
		.filter_map(|change| change["kind"].as_str())
		.collect();
	assert!(kinds.contains(&"body-modified"), "{kinds:?}");
	assert!(kinds.contains(&"moved"), "{kinds:?}");
	assert!(
		payload["ref_changes"]
			.as_array()
			.expect("ref changes")
			.iter()
			.any(|change| change["kind"] == "import-retargeted"),
		"{}",
		payload["ref_changes"]
	);
}

#[test]
fn diff_between_revisions_ignores_later_worktree_changes() {
	let tmp = moved_repo();
	git(tmp.path(), &["add", "-A"]);
	git(tmp.path(), &["commit", "-m", "move and edit"]);
	write(
		tmp.path(),
		"src/consumer.rs",
		"use crate::support::assist;\n\npub fn caller() { assist(); }\npub fn edited() -> u32 { 99 }\nfn later_noise() {}\n",
	);
	let root = tmp.path().to_str().unwrap();

	for range in ["HEAD~1..HEAD", "HEAD~1...HEAD"] {
		let (exit, out, err) = run_with(vec!["code-moniker", "diff", range, root]);

		assert_eq!(exit, Exit::Match, "{range} stderr: {err}");
		assert!(
			out.contains("src/util.rs -> src/support.rs  moved"),
			"{range}: missing move heading:\n{out}"
		);
		assert!(
			out.contains("~ fn edited()  body-modified"),
			"{range}: committed edit missing:\n{out}"
		);
		assert!(
			!out.contains("later_noise"),
			"{range}: worktree-only change must stay out of the window:\n{out}"
		);
	}
}

#[test]
fn diff_base_compares_a_revision_against_the_worktree() {
	let tmp = moved_repo();
	git(tmp.path(), &["add", "-A"]);
	git(tmp.path(), &["commit", "-m", "move and edit"]);
	write(
		tmp.path(),
		"src/consumer.rs",
		"use crate::support::assist;\n\npub fn caller() { assist(); }\npub fn edited() -> u32 { 99 }\nfn later_noise() {}\n",
	);
	let root = tmp.path().to_str().unwrap();

	let (exit, out, err) = run_with(vec!["code-moniker", "diff", root, "--base", "HEAD~1"]);

	assert_eq!(exit, Exit::Match, "stderr: {err}");
	assert!(
		out.contains("src/util.rs -> src/support.rs  moved"),
		"missing move heading:\n{out}"
	);
	assert!(
		out.contains("later_noise"),
		"worktree change must be part of base..worktree:\n{out}"
	);
}

#[test]
fn diff_rejects_an_unresolvable_revision() {
	let tmp = moved_repo();
	git(tmp.path(), &["add", "-A"]);
	git(tmp.path(), &["commit", "-m", "move and edit"]);
	let root = tmp.path().to_str().unwrap();

	let (exit, _out, err) = run_with(vec!["code-moniker", "diff", "no-such..HEAD", root]);

	assert_eq!(exit, Exit::UsageError, "stderr: {err}");
	assert!(
		err.contains("no-such"),
		"stderr must name the revision: {err}"
	);
}

#[test]
fn diff_outside_a_git_repository_reports_a_diagnostic() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/lib.rs", "fn lone() {}\n");
	let root = tmp.path().to_str().unwrap();

	let (exit, out, _err) = run_with(vec!["code-moniker", "diff", root]);

	assert_eq!(exit, Exit::Match);
	assert!(
		out.contains("diagnostic:") && out.contains("Git"),
		"missing git diagnostic:\n{out}"
	);
}
