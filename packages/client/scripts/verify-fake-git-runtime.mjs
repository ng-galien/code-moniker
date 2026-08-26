import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";

import { runGitDiffImpact } from "../dist/node.js";

const fakeGit = process.argv[2];
const supervisor = process.argv[3];
if (fakeGit === undefined || !isAbsolute(fakeGit) || supervisor === undefined || !isAbsolute(supervisor)) {
	throw new Error("usage: node verify-fake-git-runtime.mjs <absolute-fake-git> <absolute-code-moniker>");
}

const unavailableRuntime = () => ({
	async launch() { throw new Error("semantic runtime must not launch after a Git diagnostic failure"); },
});

const evidenceDirectory = await mkdtemp(join(tmpdir(), "code-moniker-node-git-runtime-"));
try {
	for (const expected of [
		{ mode: "incompatible", state: "incompatible", category: "incompatible_version", maximumMs: 2_000 },
		{ mode: "malformed", state: "unavailable", category: "malformed_version", maximumMs: 2_000 },
		{ mode: "malformed_repository", state: "unavailable", category: "malformed_output", maximumMs: 2_000 },
		{ mode: "hang", state: "timed_out", category: "timed_out", maximumMs: 4_000 },
		{ mode: "descendant", state: "timed_out", category: "timed_out", maximumMs: 4_000 },
		{ mode: "slow_rev_parse", state: "timed_out", category: "timed_out", maximumMs: 4_000 },
	]) {
		const descendantPidFile = join(evidenceDirectory, `${expected.mode}.pid`);
		const started = performance.now();
		await assert.rejects(
			runGitDiffImpact(
				{
					repository: process.cwd(),
					base: "HEAD",
					head: "HEAD",
					gitBinary: fakeGit,
					binaryCandidates: [supervisor],
					environment: {
						CODE_MONIKER_FAKE_GIT_MODE: expected.mode,
						CODE_MONIKER_FAKE_GIT_PID_FILE: descendantPidFile,
					},
				},
				unavailableRuntime,
			),
			(error) => {
				assert.equal(error.name, "GitRuntimeError");
				assert.equal(error.state, expected.state);
				assert.equal(error.diagnostic.failure?.category, expected.category);
				return true;
			},
		);
		const elapsed = performance.now() - started;
		assert.ok(elapsed < expected.maximumMs, `${expected.mode} exceeded ${expected.maximumMs} ms (${elapsed} ms)`);
		if (expected.mode === "descendant") {
			const descendantPid = Number.parseInt((await readFile(descendantPidFile, "utf8")).trim(), 10);
			await waitForProcessExit(descendantPid);
		}
	}

	await assert.rejects(
		runGitDiffImpact(
			{
				repository: process.cwd(),
				base: "HEAD",
				head: "HEAD",
				gitBinary: fakeGit,
				binaryCandidates: [fakeGit],
				environment: { CODE_MONIKER_FAKE_GIT_MODE: "incompatible" },
			},
			unavailableRuntime,
		),
		(error) => error?.diagnostic?.failure?.category === "supervisor_incompatible",
	);
	await assert.rejects(
		runGitDiffImpact(
			{
				repository: process.cwd(),
				base: "HEAD",
				head: "HEAD",
				gitBinary: fakeGit,
				binaryCandidates: [join(evidenceDirectory, "missing-code-moniker.exe")],
				environment: { CODE_MONIKER_FAKE_GIT_MODE: "incompatible" },
			},
			unavailableRuntime,
		),
		(error) => error?.diagnostic?.failure?.category === "supervisor_unavailable",
	);

	await assert.rejects(
		runGitDiffImpact(
			{
				repository: process.cwd(),
				base: "HEAD",
				head: "HEAD",
				gitBinary: fakeGit,
				binaryCandidates: [supervisor],
				environment: { CODE_MONIKER_FAKE_GIT_MODE: "large_output" },
			},
			unavailableRuntime,
		),
		/semantic runtime must not launch after a Git diagnostic failure/,
	);

	await proveSupervisorKillClosesJob(supervisor, fakeGit, evidenceDirectory);
} finally {
	await rm(evidenceDirectory, { recursive: true, force: true });
}

async function proveSupervisorKillClosesJob(supervisor, fakeGit, evidenceDirectory) {
	const pidFile = join(evidenceDirectory, "forced-supervisor-kill.pid");
	const child = spawn(supervisor, [
		"__git-runtime",
		"--executable", fakeGit,
		"--timeout-ms", "30000",
		"--output-limit", "65536",
		"--",
		"-C", process.cwd(),
		"rev-parse", "--is-inside-work-tree", "--is-bare-repository",
	], {
		env: {
			...process.env,
			CODE_MONIKER_FAKE_GIT_MODE: "descendant",
			CODE_MONIKER_FAKE_GIT_PID_FILE: pidFile,
		},
		stdio: "ignore",
		windowsHide: true,
	});
	const descendantPid = await waitForPublishedPid(pidFile);
	if (!child.kill("SIGKILL")) throw new Error("failed to terminate the Git supervisor test process");
	await waitForChildClose(child, 1_000);
	await waitForProcessExit(descendantPid);
}

function waitForChildClose(child, timeoutMs) {
	if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve();
	return new Promise((resolve, reject) => {
		const timer = setTimeout(() => finish(new Error(`Git supervisor did not close within ${timeoutMs} ms`)), timeoutMs);
		child.once("close", () => finish());
		child.once("error", finish);
		function finish(error) {
			clearTimeout(timer);
			child.removeAllListeners("close");
			child.removeAllListeners("error");
			if (error) reject(error);
			else resolve();
		}
	});
}

async function waitForPublishedPid(pidFile) {
	const deadline = Date.now() + 1_000;
	while (Date.now() <= deadline) {
		try {
			return Number.parseInt((await readFile(pidFile, "utf8")).trim(), 10);
		} catch (error) {
			if (error?.code !== "ENOENT") throw error;
		}
		await new Promise((resolve) => setTimeout(resolve, 20));
	}
	assert.fail("fake Git descendant did not publish its PID before supervisor termination");
}

console.log("Node Git runtime typed-failure checks passed");

async function waitForProcessExit(pid) {
	const deadline = Date.now() + 1_000;
	while (Date.now() <= deadline) {
		try {
			process.kill(pid, 0);
		} catch (error) {
			if (error?.code === "ESRCH") return;
			throw error;
		}
		await new Promise((resolve) => setTimeout(resolve, 20));
	}
	assert.fail(`Git descendant process ${pid} survived Node timeout cleanup`);
}
