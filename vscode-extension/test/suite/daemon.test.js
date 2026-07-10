const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const { getApi, waitFor, waitForReady } = require("./helpers");

// Feature 1: the extension connects to (or starts) the workspace daemon and lists
// it as the current daemon.
async function testDaemonView() {
	const api = await getApi();
	await waitForReady(api);

	assert.ok(api.session.endpoint, "session should expose the daemon endpoint");
	assert.notStrictEqual(
		api.session.endpoint,
		process.env.CODE_MONIKER_STALE_DAEMON_ENDPOINT,
		"session should ignore and replace a stale registry entry",
	);

	const currentNode = await waitFor(
		() => api.daemons.getChildren().find((node) => node.current),
		"the current workspace daemon to appear in the list",
	);
	assert.notStrictEqual(
		currentNode.entry.endpoint,
		process.env.CODE_MONIKER_STALE_DAEMON_ENDPOINT,
		"daemon list should show the relaunched daemon, not the stale entry",
	);
	assert.ok(currentNode.entry.pid > 0, "daemon entry should carry a pid");
	assert.ok(
		currentNode.entry.workspace_roots.length > 0,
		"daemon entry should record workspace roots",
	);

	const status = await api.session.workspaceStatus();
	assert.ok(status, "workspace status should be available");
	assert.strictEqual(status.phase, "ready", "workspace should report the ready phase");
	assert.ok(status.files >= 1, "seeded workspace should report at least one file");
	assert.ok(status.symbols >= 1, "seeded workspace should report symbols");

	await assertStaleWorkspaceQueriesRefresh(api);

	console.log("daemon view: ok");
}

async function assertStaleWorkspaceQueriesRefresh(api) {
	const libPath = path.join(api.session.workspaceRoots[0], "src", "lib.rs");
	fs.appendFileSync(libPath, "\n// force daemon staleness\n");
	await waitFor(async () => {
		const status = await api.session.workspaceStatus();
		return status?.stale ? status : undefined;
	}, "workspace to become stale after file edit");

	const topLevel = await api.symbols.getChildren();
	assert.ok(
		topLevel.some((node) => node.kind === "identity" || node.kind === "symbol"),
		"symbol tree should refresh stale daemon snapshots instead of surfacing workspace_stale",
	);
}

module.exports = { testDaemonView };
