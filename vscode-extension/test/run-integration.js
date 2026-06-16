const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");
const { execFileSync } = require("node:child_process");
const { runTests } = require("@vscode/test-electron");

async function main() {
	const extensionRoot = path.resolve(__dirname, "..");
	const repoRoot = path.resolve(extensionRoot, "..");
	const binaryPath = path.join(repoRoot, "target", "debug", "code-moniker");
	const workspaceRoot = fs.mkdtempSync(path.join(os.tmpdir(), "code-moniker-vscode-"));

	seedWorkspace(workspaceRoot, binaryPath);

	execFileSync("cargo", ["build", "-p", "code-moniker"], {
		cwd: repoRoot,
		stdio: "inherit",
	});

	await runTests({
		extensionDevelopmentPath: extensionRoot,
		extensionTestsPath: path.join(extensionRoot, "test", "suite", "index.js"),
		launchArgs: [
			workspaceRoot,
			"--disable-workspace-trust",
			"--skip-welcome",
			"--skip-release-notes",
		],
		extensionTestsEnv: {
			CODE_MONIKER_REPO: repoRoot,
			CODE_MONIKER_BINARY: binaryPath,
			CODE_MONIKER_TEST_WORKSPACE: workspaceRoot,
		},
	});
}

// Seeds the temp workspace with a small Rust file and a rule that fires
// deterministically, so the daemon-backed views have real symbols and one
// violation to assert against.
function seedWorkspace(workspaceRoot, binaryPath) {
	fs.mkdirSync(path.join(workspaceRoot, "src"), { recursive: true });
	fs.mkdirSync(path.join(workspaceRoot, ".vscode"), { recursive: true });
	// Point the extension at the freshly-built binary from activation, so the
	// daemon it auto-starts uses the same binary the harness built.
	fs.writeFileSync(
		path.join(workspaceRoot, ".vscode", "settings.json"),
		JSON.stringify({ "codeMoniker.binaryPath": binaryPath }, null, 2),
	);
	fs.writeFileSync(
		path.join(workspaceRoot, "src", "lib.rs"),
		[
			"pub struct Widget {",
			"\tpub size: u32,",
			"}",
			"",
			"impl Widget {",
			"\tpub fn new() -> Self {",
			"\t\tWidget { size: 0 }",
			"\t}",
			"",
			"\tpub fn grow(&mut self) {",
			"\t\tself.size += 1;",
			"\t}",
			"}",
			"",
			"pub fn build_widget() -> Widget {",
			"\tWidget::new()",
			"}",
			"",
			"pub fn DoThing() {",
			"\tlet _ = build_widget();",
			"}",
			"",
		].join("\n"),
	);
	fs.writeFileSync(
		path.join(workspaceRoot, ".code-moniker.toml"),
		[
			"default_rules = false",
			"",
			"[[rust.fn.where]]",
			'id = "function-snake-case"',
			'expr = "name =~ ^[a-z][a-z0-9_]*$"',
			'severity = "warn"',
			'message = "Function `{name}` should be snake_case."',
			"",
		].join("\n"),
	);
}

main().catch((error) => {
	console.error(error);
	process.exit(1);
});
