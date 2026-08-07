import assert from "node:assert/strict";
import {
	chmodSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { PROTOCOL_VERSION } from "../dist/index.js";
import {
	NodeDaemonRuntime,
	bundledBinaryPath,
	defaultBinaryCandidates,
} from "../dist/node.js";

test("the Node runtime provides portable default binary candidates", () => {
	const candidates = defaultBinaryCandidates();
	assert.equal(candidates.at(-1), "code-moniker");
	const bundled = bundledBinaryPath();
	if (bundled !== undefined) {
		assert.equal(candidates[0], bundled);
	}
});

test("the Node runtime discovers and targets exact registered workspaces", () => {
	const fixture = registryFixture();
	try {
		const runtime = new NodeDaemonRuntime({
			registryDirectory: fixture.registry,
		});
		const matching = daemonEntry({
			pid: 202,
			token: "matching",
			workspaceRoots: [fixture.root],
		});
		const configured = daemonEntry({
			pid: 303,
			token: "configured",
			workspaceRoots: [fixture.root],
			project: "named",
		});
		writeEntry(fixture.registry, "configured.json", configured);
		writeEntry(fixture.registry, "matching.json", matching);
		writeFileSync(join(fixture.registry, "invalid.json"), "{}");

		assert.deepEqual(
			runtime.listDaemons().map((entry) => entry.pid),
			[303, 202],
		);
		assert.equal(runtime.findDaemon([fixture.root])?.token, "matching");
		assert.equal(runtime.entryMatchesRoots(configured, [fixture.root]), false);
		assert.equal(runtime.entryMatchesRoots(matching, [fixture.root]), true);
	} finally {
		fixture.cleanup();
	}
});

test("Windows verbatim workspace paths match their regular form", {
	skip: process.platform !== "win32",
}, () => {
	const fixture = registryFixture();
	try {
		const runtime = new NodeDaemonRuntime({
			registryDirectory: fixture.registry,
		});
		const entry = daemonEntry({
			pid: 202,
			token: "verbatim",
			workspaceRoots: [`\\\\?\\${fixture.root}`],
		});
		assert.equal(runtime.entryMatchesRoots(entry, [fixture.root]), true);
	} finally {
		fixture.cleanup();
	}
});

test("forgetting a daemon claim is guarded by pid and token", () => {
	const fixture = registryFixture();
	try {
		const runtime = new NodeDaemonRuntime({
			registryDirectory: fixture.registry,
		});
		const entry = daemonEntry({
			pid: 202,
			token: "current",
			workspaceRoots: [fixture.root],
		});
		const file = writeEntry(fixture.registry, "claim.json", entry);

		runtime.forgetDaemon({ ...entry, token: "stale-reader" });
		assert.equal(JSON.parse(readFileSync(file, "utf8")).token, "current");

		runtime.forgetDaemon(entry);
		assert.equal(runtime.listDaemons().length, 0);
	} finally {
		fixture.cleanup();
	}
});

test("the Node runtime connects with the portable handshake and can stop an explicit daemon", async () => {
	const daemon = new FakeDaemon();
	const runtime = new NodeDaemonRuntime({
		webSocketFactory: daemon.factory,
	});
	const entry = daemonEntry({
		pid: 999_999,
		token: "remote",
		workspaceRoots: ["/workspace/project"],
	});

	const client = await runtime.connect(entry, { clientName: "node-test" });
	assert.equal(client.handshake.protocol_version, PROTOCOL_VERSION);
	assert.deepEqual(daemon.requests[0], {
		jsonrpc: "2.0",
		id: 1,
		method: "moniker_handshake",
		params: ["node-test"],
	});
	client.close();

	await runtime.stop(entry, { exitTimeoutMs: 5, pollIntervalMs: 1 });
	assert.equal(daemon.requests.at(-1).method, "moniker_shutdown");
});

test("launch uses runtime-level candidates and returns its registered ownership claim", {
	skip: process.platform === "win32",
}, async () => {
	const fixture = registryFixture();
	const binary = join(fixture.base, "fake-code-moniker.mjs");
	writeFileSync(binary, fakeDaemonScript());
	chmodSync(binary, 0o755);
	const runtime = new NodeDaemonRuntime({
		registryDirectory: fixture.registry,
		binaryCandidates: [join(fixture.base, "missing-code-moniker"), binary],
	});

	let owned;
	try {
		owned = await runtime.launch({
			workspaceRoots: [fixture.root],
			environment: {
				...process.env,
				CODE_MONIKER_TEST_REGISTRY: fixture.registry,
			},
			registrationTimeoutMs: 2_000,
			pollIntervalMs: 10,
		});

		assert.equal(owned.entry.pid, owned.process.pid);
		assert.equal(owned.entry.token, `owned-${owned.process.pid}`);
		assert.equal(runtime.findDaemon([fixture.root])?.pid, owned.process.pid);
	} finally {
		owned?.process.terminate();
		fixture.cleanup();
	}
});

test("restart confirms daemon exit before replacing its registry claim", async () => {
	const runtime = new NodeDaemonRuntime();
	const entry = daemonEntry({
		pid: 202,
		token: "current",
		workspaceRoots: ["/workspace/project"],
	});
	const launched = {
		entry: { ...entry, pid: 303, token: "replacement" },
		process: {
			pid: 303,
			isRunning: () => true,
			terminate: () => {},
		},
	};
	const calls = [];
	runtime.stop = async () => {
		calls.push("stop");
	};
	runtime.forgetDaemon = () => {
		calls.push("forget");
	};
	runtime.launch = async () => {
		calls.push("launch");
		return launched;
	};

	const replacement = await runtime.restart(entry, {
		workspaceRoots: ["/workspace/project"],
		binaryCandidates: ["code-moniker"],
	});

	assert.equal(replacement, launched);
	assert.deepEqual(calls, ["stop", "forget", "launch"]);
});

test("restart preserves the current claim when daemon shutdown fails", async () => {
	const runtime = new NodeDaemonRuntime();
	const entry = daemonEntry({
		pid: 202,
		token: "current",
		workspaceRoots: ["/workspace/project"],
	});
	const calls = [];
	runtime.stop = async () => {
		calls.push("stop");
		throw new Error("daemon did not exit");
	};
	runtime.forgetDaemon = () => {
		calls.push("forget");
	};
	runtime.launch = async () => {
		calls.push("launch");
		throw new Error("must not launch");
	};

	await assert.rejects(
		runtime.restart(entry, {
			workspaceRoots: ["/workspace/project"],
			binaryCandidates: ["code-moniker"],
		}),
		/daemon did not exit/,
	);
	assert.deepEqual(calls, ["stop"]);
});

test("owned daemon termination waits for process exit and propagates a timeout", async () => {
	const runtime = new NodeDaemonRuntime();
	const entry = daemonEntry({
		pid: 202,
		token: "owned",
		workspaceRoots: ["/workspace/project"],
	});
	const calls = [];
	const expected = new Error("daemon did not exit");
	runtime.findDaemon = () => undefined;
	runtime.waitForExit = async () => {
		calls.push("wait");
		throw expected;
	};

	await assert.rejects(
		runtime.stopOwned(
			{
				entry,
				process: {
					pid: entry.pid,
					isRunning: () => true,
					terminate: () => {
						calls.push("terminate");
					},
				},
			},
			{ exitTimeoutMs: 10, pollIntervalMs: 1 },
		),
		(error) => error === expected,
	);
	assert.deepEqual(calls, ["terminate", "wait"]);
});

function registryFixture() {
	const base = mkdtempSync(join(tmpdir(), "code-moniker-node-client-"));
	const registry = join(base, "registry");
	const root = join(base, "workspace");
	mkdirSync(registry);
	mkdirSync(root);
	return {
		base,
		registry,
		root,
		cleanup: () => rmSync(base, { recursive: true, force: true }),
	};
}

function daemonEntry({
	pid,
	token,
	workspaceRoots,
	project = null,
}) {
	return {
		workspace_root: workspaceRoots[0],
		workspace_roots: workspaceRoots,
		project,
		cache_dir: null,
		live_refresh: "on-demand",
		endpoint: "127.0.0.1:3210",
		token,
		pid,
		build: { version: "0.6.0", fingerprint: "test" },
		heartbeat_unix_ms: Date.now(),
	};
}

function writeEntry(registry, name, entry) {
	const file = join(registry, name);
	writeFileSync(file, JSON.stringify(entry));
	return file;
}

function fakeDaemonScript() {
	return `#!/usr/bin/env node
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const roots = process.argv.slice(4, process.argv.indexOf("--supervisor-pid"));
const registry = process.env.CODE_MONIKER_TEST_REGISTRY;
mkdirSync(registry, { recursive: true });
writeFileSync(join(registry, "owned.json"), JSON.stringify({
	workspace_root: roots[0],
	workspace_roots: roots,
	project: null,
	cache_dir: null,
	live_refresh: "on-demand",
	endpoint: "127.0.0.1:39999",
	token: \`owned-\${process.pid}\`,
	pid: process.pid,
	build: { version: "0.6.0", fingerprint: "fake" },
	heartbeat_unix_ms: Date.now()
}));
setInterval(() => {}, 1000);
`;
}

class FakeDaemon {
	constructor() {
		this.requests = [];
		this.factory = this.createSocket.bind(this);
	}

	createSocket() {
		this.socket = new FakeWebSocket(this);
		return this.socket;
	}

	receive(payload) {
		const request = JSON.parse(payload);
		this.requests.push(request);
		let result;
		if (request.method === "moniker_handshake") {
			result = {
						protocol_version: PROTOCOL_VERSION,
						daemon_version: "0.6.0",
						build: { version: "0.6.0", fingerprint: "test" },
						workspace_root: "/workspace/project",
						workspace_roots: ["/workspace/project"],
						capabilities: {
							queries: [],
							query_mcp_tools: {},
							commands: [],
							events: [],
						},
					};
		} else if (request.method === "moniker_query") {
			result = {
				generation: 1,
				next_cursor: null,
				result: {
					kind: "workspace_status",
					data: {
						phase: "ready",
						generation: 1,
						roots: [],
						files: 0,
						symbols: 0,
						references: 0,
						stale_summary: "fresh",
					},
				},
			};
		} else {
			result = null;
		}
		this.socket.emit("message", {
			data: JSON.stringify({
				jsonrpc: "2.0",
				id: request.id,
				result,
			}),
		});
	}
}

class FakeWebSocket {
	constructor(daemon) {
		this.daemon = daemon;
		this.listeners = new Map();
	}

	addEventListener(type, listener) {
		const listeners = this.listeners.get(type) ?? [];
		listeners.push(listener);
		this.listeners.set(type, listeners);
		if (type === "open") {
			listener({});
		}
	}

	removeEventListener(type, listener) {
		const retained = [];
		for (const candidate of this.listeners.get(type) ?? []) {
			if (candidate !== listener) {
				retained.push(candidate);
			}
		}
		this.listeners.set(type, retained);
	}

	send(payload) {
		this.daemon.receive(payload);
	}

	close() {
		this.emit("close", {});
	}

	emit(type, event) {
		for (const listener of this.listeners.get(type) ?? []) {
			listener(event);
		}
	}
}
