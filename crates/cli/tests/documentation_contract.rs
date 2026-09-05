use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pulldown_cmark::{Event, Parser, Tag};

fn workspace_root() -> Option<PathBuf> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
	root.join("docs/README.md").is_file().then_some(root)
}

fn markdown_files(root: &Path) -> Vec<PathBuf> {
	let mut files = vec![root.join("README.md")];
	collect_markdown(&root.join("docs"), &mut files);
	for readme in [
		root.join("packages/client/README.md"),
		root.join("vscode-extension/README.md"),
	] {
		if readme.is_file() {
			files.push(readme);
		}
	}
	files
}

fn collect_markdown(directory: &Path, files: &mut Vec<PathBuf>) {
	for entry in fs::read_dir(directory).expect("read documentation directory") {
		let path = entry.expect("documentation entry").path();
		if path.is_dir() {
			collect_markdown(&path, files);
		} else if path.extension().is_some_and(|extension| extension == "md") {
			files.push(path);
		}
	}
}

fn local_target(destination: &str) -> Option<&str> {
	let destination = destination.trim();
	if destination.is_empty()
		|| destination.starts_with('#')
		|| destination.starts_with('/')
		|| destination
			.split_once(':')
			.is_some_and(|(scheme, _)| !scheme.contains('/'))
	{
		return None;
	}
	let target = destination
		.split_once('#')
		.map_or(destination, |(path, _)| path);
	Some(target.split_once('?').map_or(target, |(path, _)| path))
}

#[test]
fn public_documentation_has_no_missing_local_link_targets() {
	let Some(root) = workspace_root() else {
		return;
	};
	for document in markdown_files(&root) {
		let contents = fs::read_to_string(&document)
			.unwrap_or_else(|error| panic!("{}: {error}", document.display()));
		for event in Parser::new(&contents) {
			let Event::Start(Tag::Link { dest_url, .. }) = event else {
				continue;
			};
			let Some(target) = local_target(&dest_url) else {
				continue;
			};
			let resolved = document.parent().expect("document parent").join(target);
			assert!(
				resolved.exists(),
				"{} references missing local target `{}`",
				document.display(),
				dest_url
			);
		}
	}
}

#[test]
fn documentation_index_maps_every_public_cli_command() {
	let Some(root) = workspace_root() else {
		return;
	};
	let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.arg("--help")
		.output()
		.expect("render CLI help");
	assert!(output.status.success());
	let help = String::from_utf8(output.stdout).expect("UTF-8 CLI help");
	let index = fs::read_to_string(root.join("docs/README.md")).expect("read documentation index");
	let commands = help
		.lines()
		.skip_while(|line| *line != "Commands:")
		.skip(1)
		.take_while(|line| line.starts_with("  "))
		.filter_map(|line| line.split_whitespace().next())
		.filter(|command| *command != "help");
	for command in commands {
		assert!(
			index.contains(&format!("`{command}`")),
			"documentation index does not map public CLI command `{command}`"
		);
	}
}

#[test]
fn indexed_help_surfaces_route_to_live_discovery() {
	for (command, expected) in [
		(
			"query",
			["query.describe", "daemon list", "--daemon <ENDPOINT>"],
		),
		("daemon", ["daemon list", "daemon status", "query.describe"]),
		#[cfg(feature = "mcp")]
		("mcp", ["agent install", "tools/list", "--transport"]),
	] {
		let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.args([command, "--help"])
			.output()
			.unwrap_or_else(|error| panic!("render {command} help: {error}"));
		assert!(output.status.success());
		let help = String::from_utf8(output.stdout).expect("UTF-8 command help");
		for fragment in expected {
			assert!(
				help.contains(fragment),
				"{command} help misses `{fragment}`:\n{help}"
			);
		}
	}
}

#[test]
fn bundled_documentation_is_complete_and_readable_from_a_standalone_binary() {
	let directory = tempfile::tempdir().expect("isolated directory");
	let binary = directory
		.path()
		.join(format!("code-moniker{}", std::env::consts::EXE_SUFFIX));
	fs::copy(env!("CARGO_BIN_EXE_code-moniker"), &binary).expect("copy standalone binary");
	let run = |args: &[&str]| {
		let output = Command::new(&binary)
			.current_dir(directory.path())
			.args(args)
			.output()
			.unwrap();
		assert!(
			output.status.success(),
			"{}",
			String::from_utf8_lossy(&output.stderr)
		);
		output.stdout
	};
	let inventory: serde_json::Value = serde_json::from_slice(&run(&["docs", "--json"])).unwrap();
	let pages = inventory.as_array().expect("page inventory");
	let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/docs");
	let mut expected = Vec::new();
	collect_markdown(&assets, &mut expected);
	expected.push(assets.join("schema/daemon.schema.json"));
	assert_eq!(pages.len(), expected.len());
	for source in expected {
		let path = source
			.strip_prefix(&assets)
			.unwrap()
			.to_str()
			.unwrap()
			.replace('\\', "/");
		assert!(
			pages.iter().any(|page| page["path"] == path),
			"missing bundled page {path}"
		);
		assert_eq!(
			run(&["docs", &path]),
			fs::read(&source).unwrap(),
			"bundled content differs for {path}"
		);
	}
	let query: serde_json::Value =
		serde_json::from_slice(&run(&["docs", "cli/query.md", "--json"])).unwrap();
	assert_eq!(query["path"], "cli/query.md");
	assert_eq!(
		query["body"].as_str().unwrap().as_bytes(),
		run(&["docs", "cli/query.md"])
	);
	let index = String::from_utf8(run(&["docs"])).unwrap();
	assert!(index.contains("cli/query.md") && index.contains("cli/mcp.md"));
}

#[test]
fn bundled_documentation_rejects_unknown_paths_without_reading_local_files() {
	let directory = tempfile::tempdir().unwrap();
	fs::write(directory.path().join("private.md"), "private content").unwrap();
	for path in ["missing.md", "private.md", "../README.md"] {
		let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
			.current_dir(directory.path())
			.args(["docs", path])
			.output()
			.unwrap();
		assert_eq!(output.status.code(), Some(2));
		assert!(output.stdout.is_empty());
		assert!(
			String::from_utf8(output.stderr)
				.unwrap()
				.contains("code-moniker docs")
		);
	}
}
