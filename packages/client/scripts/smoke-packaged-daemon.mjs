import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { NodeDaemonRuntime } from "@code-moniker/client/node";
import {
	assertDaemonWorkspaceIndexed,
	assertPostReadyMutation,
	seedDaemonWorkspace,
} from "./seed-daemon-workspace.mjs";

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
	seedDaemonWorkspace(workspaceRoot);
	owned = await runtime.launch({ workspaceRoots: [workspaceRoot] });
	if (owned.entry.build.fingerprint !== expectedFingerprint) {
		throw new Error(
			`packaged daemon fingerprint ${owned.entry.build.fingerprint} does not match staged binary ${expectedFingerprint}`,
		);
	}
	client = await runtime.connect(owned.entry, {
		clientName: "@code-moniker/client-packaged-smoke",
	});
	const status = await waitForReady(client);
	assertDaemonWorkspaceIndexed(status, "packaged Windows daemon");
	const mutationGeneration = await assertPostReadyMutation(
		client,
		workspaceRoot,
		status,
		"packaged Windows daemon",
	);
	const coldPid = owned.entry.pid;
	client.close();
	client = undefined;
	await runtime.stop(owned.entry, { exitTimeoutMs: 10_000 });
	assertStoppedAndUnclaimed(runtime, owned, workspaceRoot, "cold packaged daemon");
	owned = undefined;

	owned = await runtime.launch({ workspaceRoots: [workspaceRoot] });
	if (owned.entry.build.fingerprint !== expectedFingerprint) {
		throw new Error(
			`warm packaged daemon fingerprint ${owned.entry.build.fingerprint} does not match staged binary ${expectedFingerprint}`,
		);
	}
	client = await runtime.connect(owned.entry, {
		clientName: "@code-moniker/client-packaged-warm-smoke",
	});
	const warmStatus = await waitForReady(client);
	assertDaemonWorkspaceIndexed(warmStatus, "warm packaged Windows daemon");
	const warmPid = owned.entry.pid;
	client.close();
	client = undefined;
	await runtime.stop(owned.entry, { exitTimeoutMs: 10_000 });
	assertStoppedAndUnclaimed(runtime, owned, workspaceRoot, "warm packaged daemon");
	owned = undefined;
	console.log(
		`packaged daemon smoke passed: cold pid ${coldPid}, warm pid ${warmPid}, ${status.files} files, ${status.symbols} symbols, ${status.references} references, ${status.timings?.total_ms ?? "unknown"} ms, mutation generation ${mutationGeneration}`,
	);
} finally {
	client?.close();
	if (owned) {
		await runtime.stopOwned(owned, { exitTimeoutMs: 10_000 });
	}
	rmSync(workspaceRoot, { recursive: true, force: true });
	if (existsSync(workspaceRoot)) {
		throw new Error(`packaged daemon workspace cleanup failed: ${workspaceRoot}`);
	}
}

function assertStoppedAndUnclaimed(runtime, owned, workspaceRoot, label) {
	if (owned.process.isRunning()) {
		throw new Error(`${label} process ${owned.entry.pid} is still running`);
	}
	if (runtime.findDaemon([workspaceRoot])) {
		throw new Error(`${label} retained its daemon registry claim`);
	}
}

async function waitForReady(client) {
	const deadline = Date.now() + 60_000;
	while (Date.now() <= deadline) {
		const status = await client.workspace.status();
		if (
			status.phase === "ready" &&
			typeof status.generation === "number" &&
			status.generation >= 1
		) {
			return status;
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
