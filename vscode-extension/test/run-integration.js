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

main().catch((error) => {
	console.error(error);
	process.exit(1);
});
