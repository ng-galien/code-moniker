import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { NodeDaemonRuntime } from "@code-moniker/client/node";

const expectedFingerprint =
	process.env.CODE_MONIKER_EXPECTED_BINARY_FINGERPRINT;
if (!expectedFingerprint) {
	throw new Error("the packaged daemon smoke requires an expected binary fingerprint");
}

const workspaceRoot = mkdtempSync(
	join(tmpdir(), "code-moniker-packaged-client-"),
);
const runtime = new NodeDaemonRuntime();
let owned;
let client;

try {
	owned = await runtime.launch({ workspaceRoots: [workspaceRoot] });
	if (owned.entry.build.fingerprint !== expectedFingerprint) {
		throw new Error(
			`packaged daemon fingerprint ${owned.entry.build.fingerprint} does not match staged binary ${expectedFingerprint}`,
		);
	}
	client = await runtime.connect(owned.entry, {
		clientName: "@code-moniker/client-packaged-smoke",
	});
	await waitForReady(client);
	console.log(
		`packaged daemon smoke passed: pid ${owned.entry.pid}`,
	);
} finally {
	client?.close();
	if (owned) {
		await runtime.stopOwned(owned, { exitTimeoutMs: 10_000 });
	}
	rmSync(workspaceRoot, { recursive: true, force: true });
}

async function waitForReady(client) {
	const deadline = Date.now() + 60_000;
	while (Date.now() <= deadline) {
		const status = await client.workspace.status();
		if (status.phase === "ready") {
			return;
		}
		if (status.phase === "failed") {
			throw new Error(
				status.failure?.message ?? "packaged daemon failed to load its workspace",
			);
		}
		await new Promise((resolveDelay) => setTimeout(resolveDelay, 50));
	}
	throw new Error("packaged daemon workspace did not become ready");
}
