const assert = require("node:assert");
const vscode = require("vscode");

const DEFAULT_TIMEOUT_MS = 30000;

function delay(ms) {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor(probe, label, timeoutMs = DEFAULT_TIMEOUT_MS) {
	const deadline = Date.now() + timeoutMs;
	let lastError;
	while (Date.now() < deadline) {
		try {
			const value = await probe();
			if (value) {
				return value;
			}
		} catch (error) {
			lastError = error;
		}
		await delay(150);
	}
	const suffix = lastError ? ` Last error: ${lastError.message}` : "";
	throw new Error(`Timed out waiting for ${label}.${suffix}`);
}

function codeMonikerExtension() {
	const extension = vscode.extensions.all.find(
		(candidate) => candidate.packageJSON?.name === "code-moniker",
	);
	assert.ok(extension, "Code Moniker extension should be installed in the test host");
	return extension;
}

// Activates the extension and returns its exposed API. activate() is idempotent
// so calling it again returns the same instance.
async function getApi() {
	const api = await codeMonikerExtension().activate();
	assert.ok(api && api.session, "extension should expose the daemon API");
	return api;
}

// Waits until the workspace daemon is connected and indexed.
async function waitForReady(api) {
	await waitFor(() => api.session.status === "ready", "daemon to reach ready");
	return api;
}

// Flattens the identity tree into its definition nodes (breadth-first),
// pulling every level lazily through the provider. Pass `undefined` to start
// from the root.
async function collectSymbols(provider, startNode) {
	const result = [];
	const stack = [...(await provider.getChildren(startNode))];
	while (stack.length > 0) {
		const node = stack.shift();
		if (node.kind === "symbol") {
			result.push(node);
			stack.push(...(await provider.getChildren(node)));
		} else if (node.kind === "identity") {
			stack.push(...(await provider.getChildren(node)));
		}
	}
	return result;
}

module.exports = {
	delay,
	waitFor,
	getApi,
	waitForReady,
	collectSymbols,
	codeMonikerExtension,
};
