import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
	NodeDaemonRuntime,
	bundledBinaryPath,
} from "@code-moniker/client/node";

const packagedBinary = bundledBinaryPath();
if (!packagedBinary) {
	throw new Error("the native Code Moniker package is not installed");
}

const workspaceRoot = mkdtempSync(
	join(tmpdir(), "code-moniker-packaged-client-"),
);
const runtime = new NodeDaemonRuntime();
let owned;
let client;

try {
	owned = await runtime.launch({ workspaceRoots: [workspaceRoot] });
	client = await runtime.connect(owned.entry, {
		clientName: "@code-moniker/client-packaged-smoke",
	});
	await waitForReady(client);
	console.log(
		`packaged daemon smoke passed: ${packagedBinary}, pid ${owned.entry.pid}`,
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
