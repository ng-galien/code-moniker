use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn run_json(root: &Path, args: &[&str], expected_exit: i32) -> Value {
	let output = Command::new(env!("CARGO_BIN_EXE_code-moniker"))
		.current_dir(root)
		.args(args)
		.output()
		.expect("run Code Moniker");
	assert_eq!(
		output.status.code(),
		Some(expected_exit),
		"stdout: {}\nstderr: {}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).expect("read command JSON")
}

#[test]
fn check_finds_statically_identifiable_module_dependencies() {
	let project = tempfile::tempdir().expect("create project directory");
	let root = project.path();
	fs::create_dir_all(root.join("domain")).unwrap();
	fs::create_dir_all(root.join("private")).unwrap();
	fs::write(
		root.join("private/adapter.ts"),
		"export const adapter = {};\n",
	)
	.unwrap();
	fs::write(root.join("package.json"), r#"{"type":"module"}"#).unwrap();
	fs::write(
		root.join("domain/entry.ts"),
		r#"import { createRequire as create } from "node:module";
import { fileURLToPath } from "node:url";
import adapter = require("adapter");
export * as other from "../private/adapter.js";
export const load = () => import("adapter");
type Adapter = import("adapter").Adapter;
type Factory = typeof import("../private/adapter.js");
export const lazy = () => require("adapter");
function nodeLoader() {
  const loadModule = create(fileURLToPath(import.meta.url));
  return loadModule("../private/adapter.js");
}
function parameter(require: (name: string) => unknown) { return require("adapter"); }
function block() { { const require = (name: string) => name; require("adapter"); } }
function afterDeclaration() { require("adapter"); const require = (name: string) => name; }
function hoisted() { require("adapter"); if (true) { var require = (name: string) => name; } }
function fakeFactory(create: (url: string) => (name: string) => unknown) {
  const require = create(import.meta.url);
  require("adapter");
}
function otherDirectory() { const require = create('/elsewhere/index.js'); require("../private/adapter.js"); }
export const computed = (path: string) => [import(path), import(`adapter/${path}`), require(path)];
export const plain = "require('adapter')";
// import('adapter'); require('adapter');
function compatible() { { const require = (name: string) => name; } return require("adapter"); }
const escaped = import("\x61dapter");
const template = import(`adapter`);
const indexed = table[require(("adapter"))];
function defaultArgument(load = import("adapter")) { return load; }
"#,
	)
	.unwrap();
	fs::write(
		root.join("domain/actual.cjs"),
		"module.exports = require('adapter');\n",
	)
	.unwrap();
	fs::write(
		root.join("bootstrap.ts"),
		"import 'adapter';\nimport('adapter');\n",
	)
	.unwrap();
	fs::write(
		root.join(".code-moniker.toml"),
		r#"
default_rules = false
[[refs.where]]
id = "domain-dependency-avoids-adapters"
expr = """
source ~ '**/dir:domain/**' AND (kind = 'imports_symbol' OR kind = 'imports_module' OR kind = 'reexports')
=> NOT (target.segment('external_pkg') = 'adapter' OR target ~ '**/dir:private/module:adapter/**')
"""
"#,
	)
	.unwrap();

	let checked = run_json(root, &["check", ".", "--report", "--format", "json"], 1);
	let files = checked["files"].as_array().unwrap();
	let lines = |suffix: &str| {
		let file = files
			.iter()
			.find(|file| file["file"].as_str().unwrap().ends_with(suffix))
			.unwrap();
		let mut lines: Vec<_> = file["violations"]
			.as_array()
			.unwrap()
			.iter()
			.map(|violation| violation["lines"][0].as_u64().unwrap())
			.collect();
		lines.sort_unstable();
		lines
	};
	assert_eq!(
		lines("entry.ts"),
		[3, 4, 5, 6, 7, 8, 11, 25, 26, 27, 28, 29]
	);
	assert_eq!(lines("actual.cjs"), [1]);
	assert_eq!(checked["summary"]["total_violations"], 13);
}

#[test]
fn extract_visits_an_assigned_arrow_and_its_expression_callback_once() {
	let project = tempfile::tempdir().expect("create project directory");
	let root = project.path();
	fs::write(
		root.join("entry.ts"),
		"import { configure, load } from 'adapter';\nconst run = () => {\n  configure(() => load());\n};\n",
	)
	.unwrap();
	let extracted = run_json(
		root,
		&["extract", ".", "--kind", "calls", "--format", "json"],
		0,
	);
	let refs = extracted["files"][0]["matches"]["refs"].as_array().unwrap();
	assert_eq!(refs.len(), 2, "{refs:#?}");
	for target in ["/function:configure", "/function:load"] {
		let reference = refs
			.iter()
			.find(|reference| reference["target"].as_str().unwrap().ends_with(target))
			.unwrap();
		assert_eq!(reference["lines"][0], 3);
	}
}
