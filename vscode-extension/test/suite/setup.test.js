const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vscode = require("vscode");

const { getApi, waitFor } = require("./helpers");

// The Setup section answers "what is this workspace missing" for a folder that
// was never initialized, and the rows have to be actionable from the tree.
async function testSetupSection() {
	const api = await getApi();
	assert.ok(api.setup, "extension api should expose the setup provider");

	const sections = await api.workspace.getChildren(undefined);
	const setupSection = sections.find((node) => node.kind === "section" && node.id === "setup");
	assert.ok(setupSection, "workspace tree should expose a Setup section");
	assert.strictEqual(sections[0], setupSection, "Setup should be the first section");

	const rows = await api.workspace.getChildren(setupSection);
	const kinds = rows.map((row) => row.node.kind);
	assert.ok(kinds.includes("cli"), `Setup should report the CLI, got ${kinds.join(", ")}`);
	assert.ok(kinds.includes("rules"), `Setup should report the rules file, got ${kinds.join(", ")}`);
	assert.strictEqual(
		kinds.filter((kind) => kind === "agent").length,
		3,
		"Setup should report one row per agent client",
	);

	const cli = rows.find((row) => row.node.kind === "cli").node.cli;
	assert.ok(cli.version, `Setup should resolve the CLI version, got ${JSON.stringify(cli)}`);

	// The seeded workspace ships a rules file, so the row must read as present
	// and open it on click rather than offering to create one.
	const rulesRow = rows.find((row) => row.node.kind === "rules").node;
	assert.strictEqual(rulesRow.present, true, "seeded workspace has a .code-moniker.toml");
	const rulesItem = api.workspace.getTreeItem(rows.find((row) => row.node.kind === "rules"));
	assert.strictEqual(rulesItem.contextValue, "cmSetupRulesPresent");
	assert.strictEqual(rulesItem.command.command, "vscode.open");

	// No agent integration was installed in the temp workspace: every client
	// row must offer installation instead of claiming to be configured.
	for (const row of rows.filter((candidate) => candidate.node.kind === "agent")) {
		const item = api.workspace.getTreeItem(row);
		assert.strictEqual(
			item.contextValue,
			"cmSetupAgentMissing",
			`${row.node.client} should read as not installed, got ${item.description}`,
		);
	}

	await assertRulesRowFollowsDisk(api, setupSection);

	console.log(`setup section: ok (${rows.length} rows, cli ${cli.version})`);
}

// Deleting the rules file outside the editor must flip the row to its
// actionable state, and running the command the row carries must actually
// create the file — the point of the section is that nothing needs a terminal.
async function assertRulesRowFollowsDisk(api, setupSection) {
	const workspaceRoot = process.env.CODE_MONIKER_TEST_WORKSPACE;
	const rulesPath = path.join(workspaceRoot, ".code-moniker.toml");
	const saved = fs.readFileSync(rulesPath, "utf8");
	fs.unlinkSync(rulesPath);
	try {
		const missing = await waitFor(async () => {
			const rows = await api.workspace.getChildren(setupSection);
			const row = rows.find((candidate) => candidate.node.kind === "rules");
			return row && row.node.present === false ? row : undefined;
		}, "the rules row to report the deleted file");
		const item = api.workspace.getTreeItem(missing);
		assert.strictEqual(item.contextValue, "cmSetupRulesMissing");
		assert.strictEqual(item.command.command, "codeMoniker.setup.initRules");

		await vscode.commands.executeCommand(item.command.command);
		assert.ok(fs.existsSync(rulesPath), "initRules should create the project rules file");
		const restored = await waitFor(async () => {
			const rows = await api.workspace.getChildren(setupSection);
			const row = rows.find((candidate) => candidate.node.kind === "rules");
			return row && row.node.present === true ? row : undefined;
		}, "the rules row to report the created file");
		assert.strictEqual(
			api.workspace.getTreeItem(restored).contextValue,
			"cmSetupRulesPresent",
		);
		console.log("setup rules action: ok (initRules created the file)");
	} finally {
		fs.writeFileSync(rulesPath, saved);
		api.setup.refresh();
	}
}

module.exports = { testSetupSection };
