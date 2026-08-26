import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, realpathSync, rmSync } from "node:fs";
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
	seedGitRepository(workspaceRoot);
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
	assertGitRuntimeProjected(status);
	console.log(`workspace.status git runtime: ${JSON.stringify(status.runtime_dependencies[0])}`);
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

function assertGitRuntimeProjected(status) {
	const git = status.runtime_dependencies?.find((dependency) => dependency.name === "git");
	const expectedExecutable = process.env.CODE_MONIKER_CI_GIT;
	const expectedVersion = process.env.CODE_MONIKER_EXPECTED_GIT_VERSION;
	if (
		git?.state !== "available"
		|| git.resolution_source !== (expectedExecutable === undefined ? "inherited_path" : "explicit_configuration")
		|| typeof git.executable !== "string"
		|| !/^git version \d+\.\d+\.\d+/.test(git.version ?? "")
		|| git.compatible !== true
		|| !git.roots?.some((root) => root.state === "worktree")
		|| (expectedExecutable !== undefined && normalizedPath(git.executable) !== normalizedPath(realpathSync(expectedExecutable)))
		|| (expectedVersion !== undefined && git.version !== expectedVersion)
	) {
		throw new Error(`workspace.status did not project the usable Git runtime: ${JSON.stringify(git)}`);
	}
}

function seedGitRepository(root) {
	const git = process.env.CODE_MONIKER_CI_GIT ?? "git";
	const run = (...args) => execFileSync(git, ["-C", root, ...args], { stdio: "ignore" });
	run("init", "--quiet");
	run("config", "user.email", "code-moniker@example.test");
	run("config", "user.name", "Code Moniker");
	run("add", ".");
	run("commit", "--quiet", "-m", "daemon smoke base");
}

function normalizedPath(path) {
	return path.replace(/^\\\\\?\\/, "").replaceAll("\\", "/").toLowerCase();
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
		const git = status.runtime_dependencies?.find((dependency) => dependency.name === "git");
		if (
			status.phase === "ready" &&
			typeof status.generation === "number" &&
			status.generation >= 1 &&
			git !== undefined &&
			git.state !== "checking"
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
