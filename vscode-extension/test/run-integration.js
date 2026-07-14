const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");
const { execFileSync } = require("node:child_process");
const { runTests } = require("@vscode/test-electron");

async function main() {
	const extensionRoot = path.resolve(__dirname, "..");
	const repoRoot = path.resolve(extensionRoot, "..");
	const binaryName = process.platform === "win32" ? "code-moniker.exe" : "code-moniker";
	const binaryPath = path.join(repoRoot, "target", "debug", binaryName);
	const bundledBinary = path.join(extensionRoot, "bin", binaryName);
	const workspaceRoot = fs.mkdtempSync(path.join(os.tmpdir(), "code-moniker-vscode-"));

	execFileSync("cargo", ["build", "-p", "code-moniker"], {
		cwd: repoRoot,
		stdio: "inherit",
	});
	const stagedBundle = !fs.existsSync(bundledBinary);
	if (stagedBundle) {
		fs.mkdirSync(path.dirname(bundledBinary), { recursive: true });
		fs.copyFileSync(binaryPath, bundledBinary);
		if (process.platform !== "win32") {
			fs.chmodSync(bundledBinary, 0o755);
		}
	}
	seedWorkspace(workspaceRoot);

	try {
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
				CODE_MONIKER_BINARY: bundledBinary,
				CODE_MONIKER_TEST_WORKSPACE: workspaceRoot,
			},
		});
	} finally {
		if (stagedBundle) {
			fs.rmSync(path.dirname(bundledBinary), { recursive: true, force: true });
		}
	}
}

// Seeds the temp workspace with a small Rust file and a rule that fires
// deterministically, so the daemon-backed views have real symbols and one
// violation to assert against.
function seedWorkspace(workspaceRoot) {
	fs.mkdirSync(path.join(workspaceRoot, "src"), { recursive: true });
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
