const assert = require("node:assert");

const { getApi, waitFor, waitForReady } = require("./helpers");

// Feature 1: the extension connects to (or starts) the workspace daemon and lists
// it as the current daemon.
async function testDaemonView() {
	const api = await getApi();
	await waitForReady(api);

	assert.ok(api.session.endpoint, "session should expose the daemon endpoint");

	const currentNode = await waitFor(
		() => api.daemons.getChildren().find((node) => node.current),
		"the current workspace daemon to appear in the list",
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

	console.log("daemon view: ok");
}

module.exports = { testDaemonView };
